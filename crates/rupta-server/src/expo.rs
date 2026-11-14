//! Writing the Prometheus text exposition format (version 0.0.4), and a
//! strict checker of it for the tests.

use std::fmt::Write as _;

/// A metrics page under construction.
#[derive(Default)]
pub struct Expo {
    out: String,
}

/// A sample value: integers without a fraction, other numbers as Rust
/// prints them, infinities as `+Inf`/`-Inf`.
pub fn value(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "+Inf".into() } else { "-Inf".into() }
    } else if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

impl Expo {
    /// Starts a metric family: `# HELP` and `# TYPE`.
    pub fn family(&mut self, name: &str, kind: &str, help: &str) {
        let help = help.replace('\\', "\\\\").replace('\n', "\\n");
        let _ = write!(self.out, "# HELP {name} {help}\n# TYPE {name} {kind}\n");
    }

    /// One sample.
    pub fn sample(&mut self, name: &str, labels: &[(&str, &str)], v: f64) {
        self.out.push_str(name);
        if !labels.is_empty() {
            self.out.push('{');
            for (i, (k, val)) in labels.iter().enumerate() {
                if i > 0 {
                    self.out.push(',');
                }
                let _ = write!(self.out, "{k}=\"{}\"", rupta_api::state::escape_label(val));
            }
            self.out.push('}');
        }
        let _ = writeln!(self.out, " {}", value(v));
    }

    /// A family with one unlabeled sample.
    pub fn single(&mut self, name: &str, kind: &str, help: &str, v: f64) {
        self.family(name, kind, help);
        self.sample(name, &[], v);
    }

    /// Text rendered elsewhere in the same format.
    pub fn raw(&mut self, text: &str) {
        self.out.push_str(text);
        if !text.is_empty() && !text.ends_with('\n') {
            self.out.push('\n');
        }
    }

    pub fn finish(self) -> String {
        self.out
    }
}

/// Checks a page strictly: every family declared once with `# TYPE`
/// before its samples, samples of a family together, valid names, labels
/// and values, no series twice, counters ending in `_total`, histograms
/// with cumulative buckets that end in `+Inf` equal to `_count`. Returns
/// the families and their types.
#[cfg(test)]
pub fn check(text: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    use std::collections::{BTreeMap, HashSet};
    let name_ok = |n: &str| {
        let mut c = n.chars();
        c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_' || f == ':')
            && c.all(|x| x.is_ascii_alphanumeric() || x == '_' || x == ':')
    };
    let mut types: BTreeMap<String, String> = BTreeMap::new();
    let mut helps: HashSet<String> = HashSet::new();
    let mut finished: HashSet<String> = HashSet::new();
    let mut current: Option<String> = None;
    let mut series: HashSet<String> = HashSet::new();
    // (family, labels without le) -> (last bucket value, saw +Inf value)
    let mut buckets: BTreeMap<(String, String), (f64, Option<f64>)> = BTreeMap::new();
    let mut counts: BTreeMap<(String, String), f64> = BTreeMap::new();
    for (no, line) in text.lines().enumerate() {
        let err = |m: String| Err(format!("line {}: {m}: {line:?}", no + 1));
        if line.is_empty() {
            return err("empty line".into());
        }
        if let Some(rest) = line.strip_prefix("# HELP ") {
            let name = rest.split(' ').next().unwrap_or_default();
            if !helps.insert(name.to_string()) {
                return err(format!("second HELP for {name}"));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("# TYPE ") {
            let mut p = rest.split(' ');
            let (name, kind) = (p.next().unwrap_or_default(), p.next().unwrap_or_default());
            if !name_ok(name) {
                return err(format!("bad metric name {name}"));
            }
            if !matches!(kind, "counter" | "gauge" | "histogram" | "summary" | "untyped") {
                return err(format!("bad type {kind}"));
            }
            if types.insert(name.to_string(), kind.to_string()).is_some() {
                return err(format!("second TYPE for {name}"));
            }
            if kind == "counter" && !name.ends_with("_total") {
                return err(format!("counter {name} does not end in _total"));
            }
            if let Some(c) = current.replace(name.to_string()) {
                finished.insert(c);
            }
            continue;
        }
        if line.starts_with('#') {
            return err("unknown comment".into());
        }
        let (series_part, val) = line.rsplit_once(' ').ok_or_else(|| format!("line {}: no value", no + 1))?;
        let v: f64 = match val {
            "+Inf" => f64::INFINITY,
            "-Inf" => f64::NEG_INFINITY,
            "NaN" => f64::NAN,
            x => x.parse().map_err(|_| format!("line {}: bad value {x}", no + 1))?,
        };
        let (name, labels) = match series_part.split_once('{') {
            Some((n, l)) => (n, l.strip_suffix('}').ok_or_else(|| format!("line {}: unclosed labels", no + 1))?),
            None => (series_part, ""),
        };
        if !name_ok(name) {
            return err(format!("bad sample name {name}"));
        }
        // Labels: k="v" pairs with escapes.
        let mut parsed: Vec<(String, String)> = Vec::new();
        let mut rest = labels;
        while !rest.is_empty() {
            let (k, after) = rest.split_once("=\"").ok_or_else(|| format!("line {}: bad label", no + 1))?;
            if !name_ok(k) || k.contains(':') {
                return err(format!("bad label name {k}"));
            }
            let mut val = String::new();
            let mut chars = after.char_indices();
            let mut end = None;
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => match chars.next() {
                        Some((_, 'n')) => val.push('\n'),
                        Some((_, '\\')) => val.push('\\'),
                        Some((_, '"')) => val.push('"'),
                        _ => return err("bad escape".into()),
                    },
                    '"' => {
                        end = Some(i);
                        break;
                    }
                    '\n' => return err("newline in label".into()),
                    c => val.push(c),
                }
            }
            let end = end.ok_or_else(|| format!("line {}: unterminated label value", no + 1))?;
            parsed.push((k.to_string(), val));
            rest = &after[end + 1..];
            rest = rest.strip_prefix(',').unwrap_or(rest);
        }
        let family = types
            .keys()
            .filter(|f| {
                name == f.as_str()
                    || (types[*f] == "histogram"
                        && [format!("{f}_bucket"), format!("{f}_sum"), format!("{f}_count")].iter().any(|s| s == name))
            })
            .max_by_key(|f| f.len())
            .cloned()
            .ok_or_else(|| format!("line {}: sample {name} without TYPE", no + 1))?;
        if current.as_deref() != Some(&family) {
            return err(format!("sample of {family} outside its block"));
        }
        if finished.contains(&family) {
            return err(format!("{family} appears twice"));
        }
        if !series.insert(series_part.to_string()) {
            return err("duplicate series".into());
        }
        if types[&family] == "histogram" {
            let key_labels: Vec<String> =
                parsed.iter().filter(|(k, _)| k != "le").map(|(k, v)| format!("{k}={v}")).collect();
            let key = (family.clone(), key_labels.join(","));
            if name.ends_with("_bucket") {
                let le = parsed
                    .iter()
                    .find(|(k, _)| k == "le")
                    .ok_or_else(|| format!("line {}: bucket without le", no + 1))?;
                let e = buckets.entry(key).or_insert((0.0, None));
                if v < e.0 {
                    return err("buckets are not cumulative".into());
                }
                e.0 = v;
                if le.1 == "+Inf" {
                    e.1 = Some(v);
                }
            } else if name.ends_with("_count") {
                counts.insert(key, v);
            }
        } else if (types[&family] == "counter" || types[&family] == "gauge") && name != family {
            return err("sample name differs from its family".into());
        }
    }
    for (key, (_, inf)) in &buckets {
        let inf = inf.ok_or_else(|| format!("{key:?}: no +Inf bucket"))?;
        if counts.get(key) != Some(&inf) {
            return Err(format!("{key:?}: +Inf bucket differs from _count"));
        }
    }
    Ok(types)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_checks() {
        let mut e = Expo::default();
        e.single("a_total", "counter", "A.", 3.0);
        e.family("b", "gauge", "B.");
        e.sample("b", &[("x", "q\"uo\\te\nd")], 0.25);
        e.family("h_seconds", "histogram", "H.");
        e.sample("h_seconds_bucket", &[("le", "1")], 1.0);
        e.sample("h_seconds_bucket", &[("le", "+Inf")], 2.0);
        e.sample("h_seconds_sum", &[], 1.5);
        e.sample("h_seconds_count", &[], 2.0);
        let s = e.finish();
        assert!(s.contains("b{x=\"q\\\"uo\\\\te\\nd\"} 0.25\n"), "{s}");
        let t = check(&s).unwrap();
        assert_eq!(t["h_seconds"], "histogram");
    }

    #[test]
    fn rejects_bad_pages() {
        assert!(check("# TYPE a counter\na 1\n").is_err(), "counter without _total");
        assert!(check("x_total 1\n").is_err(), "no TYPE");
        assert!(check("# TYPE a_total counter\na_total 1\na_total 2\n").is_err(), "duplicate");
        assert!(check("# TYPE a gauge\n# TYPE b gauge\nb 1\na 1\n").is_err(), "outside its block");
        assert!(check("# TYPE h histogram\nh_bucket{le=\"1\"} 2\nh_bucket{le=\"+Inf\"} 1\nh_count 1\n").is_err());
        assert!(check("# TYPE h histogram\nh_bucket{le=\"+Inf\"} 2\nh_count 1\n").is_err());
        assert!(check("# TYPE g gauge\ng{a=\"x} 1\n").is_err());
        assert_eq!(value(2.0), "2");
        assert_eq!(value(0.5), "0.5");
        assert_eq!(value(f64::INFINITY), "+Inf");
    }
}
