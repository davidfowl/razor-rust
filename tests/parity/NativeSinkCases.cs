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
        public List<int> Flushes { get; } = [];
        public override bool CanGetUnflushedBytes => _native;
        public override long UnflushedBytes => _native ? Pending : throw new NotSupportedException();

        public override void Advance(int bytes)
        {
            if (FailNextAdvance)
            {
                FailNextAdvance = false;
                var accepted = Math.Min(1, bytes);
                _buffer.Advance(accepted);
                Pending += accepted;
                throw new IOException("original partial failure");
            }
            _buffer.Advance(bytes);
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
            return ValueTask.FromResult(new FlushResult(isCanceled: false, isCompleted: false));
        }
    }
}
