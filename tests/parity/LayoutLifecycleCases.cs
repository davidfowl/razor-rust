using System.Buffers;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;

static class LayoutLifecycleCases
{
    public static async Task Run()
    {
        foreach (var held in new[] { false, true })
        foreach (var failAt in new[] { -1, 0, 1, 2, 3, 4 })
        {
            var text = await Render(false, held, failAt);
            var pipe = await Render(true, held, failAt);
            if (text != pipe)
                throw new InvalidOperationException($"Text/Pipe lifecycle mismatch:\n{text}\n{pipe}");
            Console.WriteLine(text);
        }
    }

    private static async Task<string> Render(bool pipe, bool held, int failAt)
    {
        var state = new State();
        if (held) state.Gate = new(TaskCreationOptions.RunContinuationsAsynchronously);
        var page = new Page(state) { Model = state };
        var destination = new Destination(pipe, failAt);
        var status = "ok";
        try
        {
            var render = pipe ? page.RenderAsync(destination.Pipe!) : page.RenderAsync(destination.Text!);
            if (state.EnteredGate)
            {
                if (render.IsCompleted)
                    throw new InvalidOperationException("Body execution must wait for the explicit gate.");
                state.Gate!.SetResult(true);
            }
            await render;
        }
        catch (IOException error) when (ReferenceEquals(error, destination.Failure))
        {
            status = "same-error";
        }
        var label = $"{(held ? "held" : "sync")}/{(failAt < 0 ? "success" : $"error-{failAt}")}";
        var common = $"common/{label}: status={status};output={destination.Output};" +
            $"local={string.Join(',', state.Locals)};gate={(state.EnteredGate ? 1 : 0)}";
        var automatic = state.Snapshot();
        page.Dispose();
        var afterPage = state.Snapshot();
        state.Outer!.Dispose();
        var afterOuter = state.Snapshot();
        if (state.Leases.Any(count => count != 1))
            throw new InvalidOperationException("Explicit outer cleanup must release each test lease once.");
        var beforeEnd = destination.Ends;
        destination.End();
        return $"{common}\ncsharp/{label}: automatic={automatic};after-page={afterPage};" +
            $"after-outer={afterOuter};writer-end={beforeEnd}->{destination.Ends}";
    }

    private sealed class State
    {
        public readonly int[] Calls = new int[3];
        public readonly int[] Leases = new int[3];
        public readonly int[] Locals = new int[3];
        public TaskCompletionSource<bool>? Gate;
        public bool EnteredGate;
        public RazorSlice? Outer;
        public string Snapshot() => $"{string.Join(',', Calls)}/{string.Join(',', Leases)}";
    }

    private sealed class Lease(State state, int node, bool owned = false) : IDisposable
    {
        private bool _released;
        public void Dispose()
        {
            if (_released) return;
            _released = true;
            (owned ? state.Leases : state.Locals)[node]++;
        }
    }

    private sealed class Page(State state) : RazorSlice<State>, IUsesLayout<InnerProxy, State>
    {
        private readonly Lease _owned = new(state, 0, true);
        public State LayoutModel => Model;
        public override Task ExecuteAsync() => Model.Gate is { } gate
            ? ExecuteHeld(gate.Task) : ExecuteNow();
        private Task ExecuteNow()
        {
            using var local = new Lease(Model, 0);
            WriteLiteral("P");
            return Task.CompletedTask;
        }
        private async Task ExecuteHeld(Task gate)
        {
            using var local = new Lease(Model, 0);
            Model.EnteredGate = true;
            await gate;
            WriteLiteral("P");
        }
        public override void Dispose()
        {
            Model.Calls[0]++;
            _owned.Dispose();
            base.Dispose();
        }
    }

    private sealed class Inner(State state) : RazorLayoutSlice<State>, IUsesLayout<OuterProxy, State>
    {
        private readonly Lease _owned = new(state, 1, true);
        public State LayoutModel => Model;
        public override async Task ExecuteAsync()
        {
            using var local = new Lease(Model, 1);
            WriteLiteral("I");
            await RenderBodyAsync();
            WriteLiteral("i");
        }
        public override void Dispose()
        {
            Model.Calls[1]++;
            _owned.Dispose();
            base.Dispose();
        }
    }

    private sealed class Outer(State state) : RazorLayoutSlice<State>
    {
        private readonly Lease _owned = new(state, 2, true);
        public override async Task ExecuteAsync()
        {
            using var local = new Lease(Model, 2);
            WriteLiteral("O");
            await RenderBodyAsync();
            WriteLiteral("o");
        }
        public override void Dispose()
        {
            Model.Calls[2]++;
            _owned.Dispose();
            base.Dispose();
        }
    }

    private sealed class InnerProxy : IRazorSliceProxy<State>
    {
        public static RazorSlice<State> CreateSlice(State state) => new Inner(state) { Model = state };
    }

    private sealed class OuterProxy : IRazorSliceProxy<State>
    {
        public static RazorSlice<State> CreateSlice(State state)
        {
            var outer = new Outer(state) { Model = state };
            state.Outer = outer;
            return outer;
        }
    }

    private sealed class Destination
    {
        public readonly IOException Failure = new("original layout lifecycle failure");
        public readonly int FailAt;
        public readonly Text? Text;
        public readonly Writer? Pipe;
        public int Ends;
        public Destination(bool pipe, int failAt)
        {
            FailAt = failAt;
            if (pipe) Pipe = new Writer(this);
            else Text = new Text(this);
        }
        public string Output => Pipe is { } pipe
            ? Encoding.UTF8.GetString(pipe.Buffer.WrittenSpan) : Text!.ToString();
        public void End()
        {
            if (Pipe is { } pipe) pipe.Complete();
            else Text!.Dispose();
        }
    }

    private sealed class Text(Destination destination) : StringWriter
    {
        public override void Write(string? value)
        {
            if (GetStringBuilder().Length == destination.FailAt) throw destination.Failure;
            base.Write(value);
        }
        protected override void Dispose(bool disposing)
        {
            if (disposing) destination.Ends++;
            base.Dispose(disposing);
        }
    }

    private sealed class Writer(Destination destination) : PipeWriter
    {
        public readonly ArrayBufferWriter<byte> Buffer = new();
        public override void Advance(int bytes) => Buffer.Advance(bytes);
        public override Memory<byte> GetMemory(int sizeHint = 0)
            => Buffer.WrittenCount == destination.FailAt ? throw destination.Failure : Buffer.GetMemory(sizeHint);
        public override Span<byte> GetSpan(int sizeHint = 0)
            => Buffer.WrittenCount == destination.FailAt ? throw destination.Failure : Buffer.GetSpan(sizeHint);
        public override ValueTask<FlushResult> FlushAsync(CancellationToken cancellationToken = default)
            => ValueTask.FromResult(new FlushResult(false, false));
        public override void CancelPendingFlush() => throw new NotSupportedException();
        public override void Complete(Exception? exception = null) => destination.Ends++;
    }
}
