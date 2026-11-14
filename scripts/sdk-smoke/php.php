<?php
// Sends one of everything the PHP SDK supports to SMOKE_DSN: errors,
// messages, attachments, check-ins, a transaction, logs and metrics.
// Profiling needs the excimer extension and is skipped without it.
require __DIR__ . '/vendor/autoload.php';

\Sentry\init([
    'dsn' => getenv('SMOKE_DSN'),
    'release' => 'smoke-php@1.0.0',
    'environment' => 'smoke',
    'traces_sample_rate' => 1.0,
    'send_default_pii' => true,
    'server_name' => 'smoke-host',
    'enable_logs' => true,
    'profiles_sample_rate' => extension_loaded('excimer') ? 1.0 : null,
]);

\Sentry\configureScope(function (\Sentry\State\Scope $scope): void {
    $scope->setUser(['id' => 'php-user', 'email' => 'php@example.com']);
    $scope->setTag('smoke', 'php');
    $scope->addAttachment(\Sentry\Attachment\Attachment::fromBytes('php.txt', 'hello from php', 'text/plain'));
});
\Sentry\addBreadcrumb(new \Sentry\Breadcrumb(\Sentry\Breadcrumb::LEVEL_INFO, \Sentry\Breadcrumb::TYPE_DEFAULT, 'smoke', 'php breadcrumb'));

function failing_function(): void
{
    throw new \RuntimeException('smoke php error');
}

try {
    failing_function();
} catch (\Throwable $e) {
    \Sentry\captureException($e);
}
\Sentry\configureScope(function (\Sentry\State\Scope $scope): void {
    $scope->clearAttachments();
});
\Sentry\captureMessage('smoke php message', \Sentry\Severity::warning());

$config = new \Sentry\MonitorConfig(\Sentry\MonitorSchedule::crontab('*/5 * * * *'), 2, 10, 'UTC');
$checkInId = \Sentry\captureCheckIn('smoke-php-cron', \Sentry\CheckInStatus::inProgress(), null, $config);
\Sentry\captureCheckIn('smoke-php-cron', \Sentry\CheckInStatus::ok(), 0.25, null, $checkInId);

$context = \Sentry\Tracing\TransactionContext::make()->setName('smoke-php-transaction')->setOp('task');
$transaction = \Sentry\startTransaction($context);
\Sentry\SentrySdk::getCurrentHub()->setSpan($transaction);
$span = $transaction->startChild(\Sentry\Tracing\SpanContext::make()->setOp('db.query')->setDescription('SELECT 1'));
usleep(10000);
$span->finish();
\Sentry\logger()->info('smoke php log %s', ['widget'], ['smoke.kind' => 'php']);
\Sentry\logger()->error('smoke php log error');
\Sentry\traceMetrics()->count('smoke.php.requests', 5, ['route' => '/cart']);
\Sentry\traceMetrics()->distribution('smoke.php.latency', 12.5, [], \Sentry\Unit::millisecond());
$transaction->finish();
\Sentry\logger()->flush();
\Sentry\traceMetrics()->flush();

\Sentry\flush();
echo "php: sent\n";
