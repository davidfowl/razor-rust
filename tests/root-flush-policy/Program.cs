using System.Buffers;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;
using static RootPolicy;

foreach (var held in new[] { false, true })
{
    foreach (var name in new[]
    {
        "empty", "below", "at", "above", "encoded", "children",
        "root-after-child", "explicit", "layout", "prefilled-empty", "prefilled-plus-one"
    })
    {
        foreach (var policy in new[] { "pipe-native", "pipe-fallback", "text" })
        {
            var prefilled = name switch
            {
                "prefilled-empty" => Threshold,
                "prefilled-plus-one" => Threshold - 1,
                _ => 0
            };
            var expected = new string('p', prefilled) + (name switch
            {
                "empty" or "prefilled-empty" => "",
                "below" => new string('x', Threshold - 1),
                "at" or "root-after-child" or "layout" => new string('x', Threshold),
                "above" => new string('x', Threshold + 1),
                "encoded" => string.Concat(Enumerable.Repeat("&amp;", 3276)) + "xxxx",
                "children" or "explicit" => new string('x', Threshold) + "s",
                "prefilled-plus-one" => "x",
                _ => throw new InvalidOperationException(name)
            });
            var gate = held ? new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously) : null;
            RazorSlice page = name == "layout" ? new LayoutPage(gate) : new Page(name, gate);
            var pipe = new CountingPipe(policy == "pipe-native", prefilled);
            using var text = new CountingText(prefilled);
            var render = policy == "text" ? page.RenderAsync(text) : page.RenderAsync(pipe);
            if (held)
            {
                Check(!render.IsCompleted, "Held execution must not complete before release.");
                Check(pipe.Flushes.Count == 0 && text.Flushes.Count == 0,
                    "No automatic flush before successful execution.");
                gate!.SetResult();
            }
            await render;
            var actual = policy == "text" ? text.ToString() : Encoding.UTF8.GetString(pipe.Accepted.ToArray());
            Check(actual == expected, "Exact output mismatch.");
            var flushes = policy == "text" ? text.Flushes : pipe.Flushes;
            var pending = policy == "text" ? text.Pending : pipe.Pending;
            Console.WriteLine($"{(held ? "held" : "ready")}/{policy}/{name}: bytes={Encoding.UTF8.GetByteCount(actual)},flushes={(flushes.Count == 0 ? "-" : string.Join(",", flushes))},pending={pending}");
        }
    }
}

await CheckErrors();

static void Check(bool condition, string message)
{
    if (!condition)
        throw new InvalidOperationException(message);
}

static async Task CheckErrors()
{
    foreach (var held in new[] { false, true })
    {
        foreach (var textPolicy in new[] { false, true })
        {
            var original = new IOException("original template failure");
            var gate = held ? new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously) : null;
            var pipe = new CountingPipe(true, 0);
            using var text = new CountingText(0);
            var page = new FaultingPage(original, gate);
            var render = textPolicy ? page.RenderAsync(text) : page.RenderAsync(pipe);
            gate?.SetResult();
            try
            {
                await render;
                throw new InvalidOperationException("Template failure must propagate.");
            }
            catch (IOException error) when (ReferenceEquals(error, original)) { }
            Check(pipe.Flushes.Count == 0 && text.Flushes.Count == 0,
                "Failed execution must not cause a completion flush.");
        }
    }

    foreach (var cancel in new[] { false, true })
    {
        var original = new IOException("original root flush failure");
        var gate = new TaskCompletionSource<FlushResult>(TaskCreationOptions.RunContinuationsAsynchronously);
        var pipe = new CountingPipe(true, 0) { FlushGate = gate };
        var render = new Page("at", null).RenderAsync(pipe);
        Check(!render.IsCompleted && pipe.Flushes.SequenceEqual(new[] { Threshold }),
            "Root threshold flush must be awaited.");
        if (cancel)
            gate.SetCanceled(new CancellationToken(canceled: true));
        else
            gate.SetException(original);
        try
        {
            await render;
            throw new InvalidOperationException("Root flush failure must propagate.");
        }
        catch (IOException error) when (!cancel && ReferenceEquals(error, original)) { }
        catch (OperationCanceledException error) when (cancel && error.CancellationToken.IsCancellationRequested) { }
    }
}

sealed class Page(string name, TaskCompletionSource? gate) : RazorSlice
{
    public override async Task ExecuteAsync()
    {
        if (gate is not null)
            await gate.Task;
        switch (name)
        {
            case "empty":
            case "prefilled-empty":
                break;
            case "below":
                WriteLiteral(new string('x', Threshold - 1));
                break;
            case "at":
            case "above":
                WriteLiteral(new string('x', name == "at" ? Threshold : Threshold + 1));
                break;
            case "encoded":
                Write(new string('&', 3276));
                WriteLiteral("xxxx");
                break;
            case "children":
                await RenderPartialAsync(new Literal(new string('x', Threshold - 1)));
                await RenderPartialAsync(new Literal("x"));
                WriteLiteral("s");
                break;
            case "root-after-child":
                await RenderPartialAsync(new Literal(new string('x', Threshold - 1)));
                WriteLiteral("x");
                break;
            case "explicit":
                WriteLiteral(new string('x', Threshold));
                await FlushAsync();
                WriteLiteral("s");
                break;
            case "prefilled-plus-one":
                WriteLiteral("x");
                break;
            default:
                throw new InvalidOperationException(name);
        }
    }
}

sealed class Literal(string value) : RazorSlice
{
    public override Task ExecuteAsync()
    {
        WriteLiteral(value);
        return Task.CompletedTask;
    }
}

sealed class LayoutPage(TaskCompletionSource? gate) : RazorSlice, IUsesLayout<LayoutProxy>
{
    public override async Task ExecuteAsync()
    {
        if (gate is not null)
            await gate.Task;
        WriteLiteral(new string('x', Threshold / 2));
    }
}

sealed class Layout : RazorLayoutSlice
{
    public override async Task ExecuteAsync()
    {
        await RenderBodyAsync();
        WriteLiteral(new string('x', Threshold / 2));
    }
}

sealed class LayoutProxy : IRazorSliceProxy
{
    public static RazorSlice CreateSlice() => new Layout();
}

sealed class FaultingPage(IOException original, TaskCompletionSource? gate) : RazorSlice
{
    public override async Task ExecuteAsync()
    {
        WriteLiteral(new string('x', Threshold));
        if (gate is not null)
            await gate.Task;
        throw original;
    }
}

sealed class CountingText(int prefilled) : StringWriter(new StringBuilder(new string('p', prefilled)))
{
    public int Pending => GetStringBuilder().Length - Flushes.Sum();
    public List<int> Flushes { get; } = [];
    public override Task FlushAsync(CancellationToken cancellationToken)
    {
        Flushes.Add(Pending);
        return Task.CompletedTask;
    }
}

sealed class CountingPipe : PipeWriter
{
    private readonly ArrayBufferWriter<byte> _buffer = new();
    private readonly bool _native;
    public CountingPipe(bool native, int prefilled)
    {
        _native = native;
        Accepted.AddRange(Enumerable.Repeat((byte)'p', prefilled));
        Pending = prefilled;
    }

    public List<byte> Accepted { get; } = [];
    public List<int> Flushes { get; } = [];
    public int Pending { get; private set; }
    public TaskCompletionSource<FlushResult>? FlushGate { get; init; }
    public override bool CanGetUnflushedBytes => _native;
    public override long UnflushedBytes => Pending;
    public override void Advance(int bytes)
    {
        _buffer.Advance(bytes);
        Accepted.AddRange(_buffer.WrittenSpan[^bytes..].ToArray());
        Pending += bytes;
    }
    public override Memory<byte> GetMemory(int sizeHint = 0) => _buffer.GetMemory(sizeHint);
    public override Span<byte> GetSpan(int sizeHint = 0) => _buffer.GetSpan(sizeHint);
    public override void CancelPendingFlush() { }
    public override void Complete(Exception? exception = null) { }
    public override ValueTask<FlushResult> FlushAsync(CancellationToken cancellationToken = default)
    {
        Flushes.Add(Pending);
        Pending = 0;
        _buffer.Clear();
        return FlushGate is null
            ? ValueTask.FromResult(new FlushResult(isCanceled: false, isCompleted: false))
            : new(FlushGate.Task);
    }
}

static class RootPolicy
{
    public const int Threshold = 16_384;
}
