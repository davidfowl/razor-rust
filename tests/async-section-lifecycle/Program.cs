using System.Buffers;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;

foreach (var heldAt in new[] { 0, 1, 2 })
foreach (var failAt in Enumerable.Range(-1, 14))
    await Emit($"{(heldAt == 0 ? "ready" : $"held-{heldAt}")}/{(failAt < 0 ? "success" : $"error-{failAt}")}",
        heldAt, failAt);
foreach (var invocation in new[] { 1, 2 })
    await Emit($"application-{invocation}", invocation, applicationAt: invocation);
foreach (var invocation in new[] { 1, 2 })
    await Emit($"cancel-{invocation}", invocation, cancel: true);
await Emit("reuse", repeats: 2);
await Emit("bare", bare: true);

static async Task Emit(string label, int heldAt = 0, int failAt = -1,
    int applicationAt = 0, bool cancel = false, int repeats = 1, bool bare = false)
{
    var text = await Run(false, heldAt, failAt, applicationAt, cancel, repeats, bare);
    var pipe = await Run(true, heldAt, failAt, applicationAt, cancel, repeats, bare);
    if (text != pipe)
        throw new InvalidOperationException($"Text/Pipe mismatch for {label}:\n{text}\n{pipe}");
    Console.WriteLine($"common/{label}: {text.Common}");
    Console.WriteLine($"csharp/{label}: {text.Reference}");
}

static async Task<(string Common, string Reference)> Run(bool pipe, int heldAt, int failAt,
    int applicationAt, bool cancel, int repeats, bool bare)
{
    var state = new State { HeldAt = heldAt, ApplicationAt = applicationAt };
    var page = new Page(state) { Model = state };
    var destination = new Destination(pipe, failAt);
    using var cancellation = new CancellationTokenSource();
    if (state.Initialized.Any(n => n != 0) || state.Trace.Length != 0)
        throw new InvalidOperationException("Construction must not initialize or execute sections.");
    var status = "ok";
    try
    {
        for (var round = 0; round < repeats; round++)
        {
            RazorSlice root = bare ? new Bare(page) : page;
            var render = pipe
                ? root.RenderAsync(destination.Pipe!, cancellationToken: cancellation.Token)
                : root.RenderAsync(destination.Text!, cancellationToken: cancellation.Token);
            if (state.EnteredGate)
            {
                if (render.IsCompleted || !state.Live.SequenceEqual(new[] { 0, 0, 1, 1, 1 }))
                    throw new InvalidOperationException("Held inherited sections must retain exactly O/F/H locals.");
                if (cancel) cancellation.Cancel();
                else state.Release.SetResult(true);
            }
            await render.AsTask().WaitAsync(TimeSpan.FromSeconds(10));
        }
    }
    catch (IOException error) when (ReferenceEquals(error, destination.Failure))
    {
        status = "same-write-error";
    }
    catch (IOException error) when (ReferenceEquals(error, state.Failure))
    {
        status = "same-application-error";
    }
    catch (OperationCanceledException error) when (cancel && error.CancellationToken == cancellation.Token)
    {
        status = "cancelled";
    }
    if (state.Live.Any(n => n != 0))
        throw new InvalidOperationException("All render-local leases must be released after settlement.");
    var common = $"status={status};output={destination.Output};local={string.Join(',', state.Locals)};" +
        $"trace={state.Trace};gate={(state.EnteredGate ? 1 : 0)}";
    var automatic = state.Snapshot();
    foreach (var slice in state.Slices.AsEnumerable().Reverse()) slice.Dispose();
    if (!state.Created.SequenceEqual(state.Releases))
        throw new InvalidOperationException("Explicit owner cleanup must release every test-owned lease once.");
    if (destination.Ends != 0) throw new InvalidOperationException("Rendering must not own the destination.");
    destination.End();
    if (destination.Ends != 1) throw new InvalidOperationException("Caller must still own the destination.");
    return (common, $"init={string.Join(',', state.Initialized)};init-order={state.InitTrace};" +
        $"automatic={automatic};cleanup={state.Snapshot()};" +
        $"sections-after-dispose={state.FooterAfterDispose},{state.HeaderAfterDispose};writer-end=0->1");
}

sealed class State
{
    public readonly int[] Initialized = new int[3];
    public readonly int[] Created = new int[3];
    public readonly int[] Calls = new int[3];
    public readonly int[] Releases = new int[3];
    public readonly int[] Locals = new int[5];
    public readonly int[] Live = new int[5];
    public readonly StringBuilder Trace = new();
    public readonly StringBuilder InitTrace = new();
    public readonly List<RazorSlice> Slices = new();
    public readonly TaskCompletionSource<bool> Release = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public readonly IOException Failure = new("original section application failure");
    public int HeldAt, ApplicationAt, Headers, FooterAfterDispose, HeaderAfterDispose;
    public bool EnteredGate;
    public string Snapshot() => $"{string.Join(',', Calls)}/{string.Join(',', Releases)}";
    public void Register(RazorSlice slice, int node)
    {
        Created[node]++;
        Slices.Add(slice);
        slice.Initialize = (_, _, _) =>
        {
            Initialized[node]++;
            InitTrace.Append("PIO"[node]);
        };
    }
    public void Dispose(int node, ref bool disposed)
    {
        Calls[node]++;
        if (!disposed) Releases[node]++;
        disposed = true;
    }
}

sealed class Local : IDisposable
{
    private readonly State _state;
    private readonly int _node;
    public Local(State state, int node)
    {
        _state = state;
        _node = node;
        state.Live[node]++;
        state.Trace.Append("PIOFH"[node]);
    }
    public void Dispose()
    {
        _state.Live[_node]--;
        _state.Locals[_node]++;
        _state.Trace.Append("piofh"[_node]);
    }
}

sealed class Page : RazorSlice<State>, IUsesLayout<InnerProxy, State>
{
    private bool _disposed;
    public Page(State state) => state.Register(this, 0);
    public State LayoutModel => Model;
    public override Task ExecuteAsync()
    {
        using var local = new Local(Model, 0);
        WriteLiteral("P");
        return Task.CompletedTask;
    }
    protected internal override async Task ExecuteSectionAsync(string name)
    {
        if (name != "header") return;
        using var local = new Local(Model, 4);
        if (_disposed) Model.HeaderAfterDispose++;
        var invocation = ++Model.Headers;
        WriteLiteral("H");
        if (invocation == Model.HeldAt)
        {
            Model.EnteredGate = true;
            await Model.Release.Task.WaitAsync(CancellationToken);
        }
        if (invocation == Model.ApplicationAt) throw Model.Failure;
        WriteLiteral("h");
    }
    public override void Dispose()
    {
        Model.Dispose(0, ref _disposed);
        base.Dispose();
    }
}

sealed class Inner : RazorLayoutSlice<State>, IUsesLayout<OuterProxy, State>
{
    private bool _disposed;
    public Inner(State state) => state.Register(this, 1);
    public State LayoutModel => Model;
    public override async Task ExecuteAsync()
    {
        using var local = new Local(Model, 1);
        WriteLiteral("I");
        await RenderBodyAsync();
        WriteLiteral("i");
    }
    protected internal override async Task ExecuteSectionAsync(string name)
    {
        if (name != "footer") return;
        using var local = new Local(Model, 3);
        if (_disposed) Model.FooterAfterDispose++;
        WriteLiteral("F");
        await RenderSectionAsync("header");
        WriteLiteral("f");
    }
    public override void Dispose()
    {
        Model.Dispose(1, ref _disposed);
        base.Dispose();
    }
}

sealed class Outer : RazorLayoutSlice<State>
{
    private bool _disposed;
    public Outer(State state) => state.Register(this, 2);
    public override async Task ExecuteAsync()
    {
        if (Model.Initialized[0] != 1 ||
            Model.Initialized[1] != Model.Created[1] || Model.Initialized[2] != Model.Created[2])
            throw new InvalidOperationException("Declared providers must initialize before outer body execution.");
        using var local = new Local(Model, 2);
        WriteLiteral("O");
        await RenderBodyAsync();
        await RenderSectionAsync("footer");
        await RenderSectionAsync("footer");
        await RenderSectionAsync("unused");
        WriteLiteral("o");
    }
    public override void Dispose()
    {
        Model.Dispose(2, ref _disposed);
        base.Dispose();
    }
}

sealed class InnerProxy : IRazorSliceProxy<State>
{
    public static RazorSlice<State> CreateSlice(State state) => new Inner(state) { Model = state };
}
sealed class OuterProxy : IRazorSliceProxy<State>
{
    public static RazorSlice<State> CreateSlice(State state) => new Outer(state) { Model = state };
}
sealed class Bare(Page page) : RazorSlice
{
    public override async Task ExecuteAsync() => await RenderChildSliceAsync(page);
}

sealed class Destination
{
    public readonly IOException Failure = new("original section destination failure");
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
sealed class Text(Destination destination) : StringWriter
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
sealed class Writer(Destination destination) : PipeWriter
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
