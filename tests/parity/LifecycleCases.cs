using System.Buffers;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;

static class LifecycleCases
{
    public static async Task Run()
    {
        foreach (var pipe in new[] { false, true })
        foreach (var held in new[] { false, true })
        foreach (var fail in new[] { false, true })
        {
            var counts = new Counts();
            using var slice = new Slice(counts);
            slice.Initialize = (_, _, _) => counts.Initialized++;
            var first = new Destination(pipe, fail);
            var second = new Destination(pipe, false);
            var status = await Render(slice, first, held);
            var retry = await Render(slice, second, held);
            var beforeEnd = $"{first.Ends},{second.Ends}";
            var output = $"{first.Output}|{second.Output}";
            first.End();
            second.End();
            Console.WriteLine(
                $"{(pipe ? "pipe" : "text")}/{(held ? "held" : "sync")}/{(fail ? "error" : "success")}: " +
                $"status={status},{retry};init={counts.Initialized};execute={counts.Executed};" +
                $"local={counts.Locals};dispose={counts.Disposed};output={output};" +
                $"writer-end={beforeEnd}->{first.Ends},{second.Ends}");
        }
    }

    private static async Task<string> Render(Slice slice, Destination destination, bool held)
    {
        slice.Gate = held ? new(TaskCreationOptions.RunContinuationsAsynchronously) : null;
        try
        {
            var render = destination.Pipe is { } pipe
                ? slice.RenderAsync(pipe)
                : slice.RenderAsync(destination.Text!);
            if (held)
            {
                if (render.IsCompleted)
                    throw new InvalidOperationException("The render must wait for the explicit gate.");
                slice.Gate!.SetResult(true);
            }
            await render;
            return "ok";
        }
        catch (IOException error) when (ReferenceEquals(error, destination.Failure))
        {
            return "same-error";
        }
    }

    private sealed class Counts
    {
        public int Initialized;
        public int Executed;
        public int Locals;
        public int Disposed;
    }

    private sealed class Lease(Counts counts) : IDisposable
    {
        public void Dispose() => counts.Locals++;
    }

    private sealed class Slice(Counts counts) : RazorSlice
    {
        public TaskCompletionSource<bool>? Gate;

        public override Task ExecuteAsync()
        {
            counts.Executed++;
            return Gate is { } gate ? ExecuteHeld(gate.Task) : ExecuteNow();
        }

        private Task ExecuteNow()
        {
            using var lease = new Lease(counts);
            WriteLiteral("body");
            return Task.CompletedTask;
        }

        private async Task ExecuteHeld(Task gate)
        {
            using var lease = new Lease(counts);
            await gate;
            WriteLiteral("body");
        }

        public override void Dispose()
        {
            counts.Disposed++;
            base.Dispose();
        }
    }

    private sealed class Destination
    {
        public readonly IOException Failure = new("original lifecycle writer failure");
        public readonly Writer? Pipe;
        public readonly Text? Text;
        public int Ends;

        public Destination(bool pipe, bool fail)
        {
            if (pipe) Pipe = new Writer(this, fail);
            else Text = new Text(this, fail);
        }

        public string Output => Pipe is { } pipe
            ? Encoding.UTF8.GetString(pipe.Buffer.WrittenSpan)
            : Text!.ToString();

        public void End()
        {
            if (Pipe is { } pipe) pipe.Complete();
            else Text!.Dispose();
        }
    }

    private sealed class Text(Destination destination, bool fail) : StringWriter
    {
        public override void Write(string? value)
        {
            if (fail) throw destination.Failure;
            base.Write(value);
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing) destination.Ends++;
            base.Dispose(disposing);
        }
    }

    private sealed class Writer(Destination destination, bool fail) : PipeWriter
    {
        public readonly ArrayBufferWriter<byte> Buffer = new();
        public override void Advance(int bytes) => Buffer.Advance(bytes);
        public override Memory<byte> GetMemory(int sizeHint = 0)
            => fail ? throw destination.Failure : Buffer.GetMemory(sizeHint);
        public override Span<byte> GetSpan(int sizeHint = 0)
            => fail ? throw destination.Failure : Buffer.GetSpan(sizeHint);
        public override ValueTask<FlushResult> FlushAsync(CancellationToken cancellationToken = default)
            => ValueTask.FromResult(new FlushResult(false, false));
        public override void CancelPendingFlush() => throw new NotSupportedException();
        public override void Complete(Exception? exception = null) => destination.Ends++;
    }
}
