using System.Buffers;
using System.IO.Pipelines;
using RazorSlices;

static class NativeSinkCases
{
    private const int Threshold = 16_384;

    public static async Task Run()
    {
        await Case("native-prefilled-plus-one", true, Threshold - 1, "x");
        await Case("native-prefilled-empty", true, Threshold, "");
        await Case("native-below-threshold", true, Threshold - 1, "");
        await Case("fallback-prefilled-plus-one", false, Threshold - 1, "x");
        await Case("native-draining", true, 0, new string('x', Threshold), drain: true);
        await Case("fallback-draining", false, 0, new string('x', Threshold), drain: true);
        await Case("native-partial-failure", true, Threshold - 1, "xy", partialFailure: true);
        await Case("fallback-partial-failure", false, Threshold - 1, "xy", partialFailure: true);
    }

    public static async Task RunFlushOutcomes()
    {
        foreach (var held in new[] { false, true })
        {
            foreach (var (name, canceled, completed) in new[]
            {
                ("normal", false, false),
                ("canceled-flag", true, false),
                ("completed-flag", false, true),
                ("both-flags", true, true)
            })
            {
                var writer = new CountingPipeWriter(true, Threshold, false)
                {
                    Result = new FlushResult(canceled, completed),
                    HeldFlush = held ? new(TaskCreationOptions.RunContinuationsAsynchronously) : null
                };
                var render = new Text("").RenderAsync(writer);
                if (held)
                {
                    if (render.IsCompleted)
                        throw new InvalidOperationException("A held flush must block completion.");
                    writer.HeldFlush!.SetResult(writer.Result);
                }
                await render;
                Console.WriteLine($"{(held ? "held" : "ready")}-{name}: render=success,flushes={string.Join(",", writer.Flushes)}");
            }
        }

        var failure = new IOException("original flush failure");
        var failed = new CountingPipeWriter(true, Threshold, false) { Failure = failure };
        try
        {
            await new Text("").RenderAsync(failed);
            throw new InvalidOperationException("A faulted native flush must fail rendering.");
        }
        catch (IOException error) when (ReferenceEquals(error, failure))
        {
            Console.WriteLine("faulted-flush: original-error");
        }

        var canceledWriter = new CountingPipeWriter(true, Threshold, false) { CancelFlush = true };
        try
        {
            await new Text("").RenderAsync(canceledWriter);
            throw new InvalidOperationException("A canceled native flush must cancel rendering.");
        }
        catch (OperationCanceledException error) when (error.CancellationToken.IsCancellationRequested)
        {
            Console.WriteLine("canceled-flush: cancellation-error");
        }

        using var text = new CountingTextWriter();
        await new Text(new string('x', Threshold)).RenderAsync(text);
        if (text.GetStringBuilder().Length != Threshold || text.Flushes != 0)
            throw new InvalidOperationException("TextWriter must not auto-flush at the pipe threshold.");
        Console.WriteLine($"textwriter-threshold: flushes={text.Flushes}");
        await new ExplicitTextFlush().RenderAsync(text);
        Console.WriteLine($"textwriter-explicit: flushes={text.Flushes}");
    }

    public static async Task RunFlushPolicy()
    {
        foreach (var size in new[] { Threshold - 1, Threshold, Threshold + 1 })
            await PolicyCase(size.ToString(), () => new Text(new string('x', size)), size);
        await PolicyCase("children", () => new PolicyChildren(), Threshold);
        await PolicyCase("explicit", () => new PolicyExplicit(), Threshold);
        await PolicyCase("layout", () => new PolicyPage(), Threshold);
    }

    private static async Task PolicyCase(string name, Func<RazorSlice> create, int size)
    {
        var pipe = new CountingPipeWriter(true, 0, false);
        await create().RenderAsync(pipe);
        if (pipe.Accepted.Count != size || pipe.Accepted.Any(value => value != (byte)'x'))
            throw new InvalidOperationException("Pipe output must match the exact ASCII literal.");
        Console.WriteLine($"pipe-{name}: flushes={pipe.Flushes.Count}");
        using var text = new CountingTextWriter();
        await create().RenderAsync(text);
        if (text.ToString() != new string('x', size))
            throw new InvalidOperationException("Text output must match the exact ASCII literal.");
        Console.WriteLine($"text-{name}: flushes={text.Flushes}");
    }

    private sealed class PolicyChildren : RazorSlice
    {
        public override async Task ExecuteAsync()
        {
            await RenderPartialAsync(new Text(new string('x', Threshold / 2)));
            await RenderPartialAsync(new Text(new string('x', Threshold / 2)));
        }
    }

    private sealed class PolicyExplicit : RazorSlice
    {
        public override async Task ExecuteAsync()
        {
            WriteLiteral(new string('x', Threshold));
            await FlushAsync();
        }
    }

    private sealed class PolicyPage : RazorSlice, IUsesLayout<PolicyLayoutProxy>
    {
        public override Task ExecuteAsync()
        {
            WriteLiteral(new string('x', Threshold / 2));
            return Task.CompletedTask;
        }
    }

    private sealed class PolicyLayout : RazorLayoutSlice
    {
        public override async Task ExecuteAsync()
        {
            await RenderBodyAsync();
            WriteLiteral(new string('x', Threshold / 2));
        }
    }

    private sealed class PolicyLayoutProxy : IRazorSliceProxy
    {
        public static RazorSlice CreateSlice() => new PolicyLayout();
    }

    private static async Task Case(
        string name, bool native, int prefilled, string text,
        bool drain = false, bool partialFailure = false)
    {
        var writer = new CountingPipeWriter(native, prefilled, drain);
        if (partialFailure)
        {
            writer.FailNextAdvance = true;
            try
            {
                await new Text(text).RenderAsync(writer);
                throw new InvalidOperationException("The partial write must fail.");
            }
            catch (IOException error) when (error.Message == "original partial failure")
            {
                if (writer.Pending != Threshold || writer.Flushes.Count != 0)
                    throw new InvalidOperationException("Failed execution must not auto-flush.");
            }
            await new Text("").RenderAsync(writer);
        }
        else
        {
            await new Text(text).RenderAsync(writer);
        }
        var flushes = writer.Flushes.Count == 0 ? "none" : string.Join(",", writer.Flushes);
        Console.WriteLine($"{name}: flushes={flushes}");
    }

    private sealed class Text(string text) : RazorSlice
    {
        public override Task ExecuteAsync()
        {
            WriteLiteral(text);
            return Task.CompletedTask;
        }
    }

    private sealed class ExplicitTextFlush : RazorSlice
    {
        public override async Task ExecuteAsync() => await FlushAsync();
    }

    private sealed class CountingTextWriter : StringWriter
    {
        public int Flushes { get; private set; }
        public override Task FlushAsync(CancellationToken cancellationToken)
        {
            Flushes++;
            return Task.CompletedTask;
        }
    }

    private sealed class CountingPipeWriter : PipeWriter
    {
        private readonly ArrayBufferWriter<byte> _buffer = new();
        private readonly bool _native;
        private readonly bool _drain;

        public CountingPipeWriter(bool native, int prefilled, bool drain)
        {
            _native = native;
            _drain = drain;
            if (prefilled > 0)
            {
                _buffer.GetSpan(prefilled)[..prefilled].Fill((byte)'p');
                _buffer.Advance(prefilled);
            }
            Pending = prefilled;
        }

        public int Pending { get; private set; }
        public bool FailNextAdvance { get; set; }
        public FlushResult Result { get; set; } = new(isCanceled: false, isCompleted: false);
        public TaskCompletionSource<FlushResult>? HeldFlush { get; set; }
        public Exception? Failure { get; set; }
        public bool CancelFlush { get; set; }
        public List<int> Flushes { get; } = [];
        public List<byte> Accepted { get; } = [];
        public override bool CanGetUnflushedBytes => _native;
        public override long UnflushedBytes => _native ? Pending : throw new NotSupportedException();

        public override void Advance(int bytes)
        {
            if (FailNextAdvance)
            {
                FailNextAdvance = false;
                var accepted = Math.Min(1, bytes);
                _buffer.Advance(accepted);
                Accepted.AddRange(_buffer.WrittenSpan[^accepted..].ToArray());
                Pending += accepted;
                throw new IOException("original partial failure");
            }
            _buffer.Advance(bytes);
            Accepted.AddRange(_buffer.WrittenSpan[^bytes..].ToArray());
            Pending = _drain ? 0 : Pending + bytes;
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
            if (Failure is not null)
                return ValueTask.FromException<FlushResult>(Failure);
            if (CancelFlush)
                return ValueTask.FromCanceled<FlushResult>(new CancellationToken(canceled: true));
            return HeldFlush is null ? ValueTask.FromResult(Result) : new(HeldFlush.Task);
        }
    }
}
