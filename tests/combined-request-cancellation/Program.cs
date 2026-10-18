using System.Buffers;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;

foreach (var mode in new[] { "document", "bare", "section-cancel", "flush-cancel", "write-error" })
{
    var text = await Run(mode, pipe: false);
    var pipe = await Run(mode, pipe: true);
    Check(text == pipe, "The shared subset must agree for both C# destinations.");
    Console.WriteLine(text);
}

static async Task<string> Run(string mode, bool pipe)
{
    using var cancellation = new CancellationTokenSource();
    var state = new State(mode, cancellation.Token);
    RazorSlice page = mode == "bare"
        ? new PartialHost { Model = state } : new Page { Model = state };
    var writer = new CountingPipe(state);
    using var text = new CountingText(state);
    var status = "ok";
    try
    {
        var render = pipe
            ? page.RenderAsync(writer, cancellationToken: cancellation.Token)
            : page.RenderAsync(text, cancellationToken: cancellation.Token);
        if (mode is "section-cancel" or "flush-cancel")
        {
            Check(!render.IsCompleted && state.Entered.Task.IsCompleted,
                "The combined stack must be suspended at its selected boundary.");
            cancellation.Cancel();
        }
        await render;
        Check(mode is "document" or "bare", "Failure/cancellation must not become success.");
    }
    catch (OperationCanceledException error) when (
        mode is "section-cancel" or "flush-cancel" &&
        error.CancellationToken == cancellation.Token)
    {
        status = "cancelled";
    }
    catch (IOException error) when (mode == "write-error" && ReferenceEquals(error, state.Original))
    {
        status = "original-error";
    }
    var expected = mode switch
    {
        "document" => "OIHhPbi|FHhfooto",
        "bare" => "Pb",
        "section-cancel" => "OIHhPbi|FHh",
        "flush-cancel" => "OIHhPbi|FHhfoot",
        "write-error" => "OIHhPbi|FHhf",
        _ => throw new InvalidOperationException(mode)
    };
    Check(state.Output.ToString() == expected, "Exact output/prefix mismatch.");
    Check(state.Traps == 0, "Child rendering must bypass declared layouts.");
    Check(state.Flushes == (mode is "document" or "flush-cancel" ? 1 : 0),
        "No final flush may run after a failed/cancelled boundary.");
    state.Gate.TrySetResult();
    Check(state.Output.ToString() == expected, "Releasing cancelled work must not append output.");
    cancellation.Cancel();
    Check(state.Tokens.All(token => token == cancellation.Token && token.IsCancellationRequested),
        "Observed tokens must retain the original cancellation-source identity.");
    return $"{mode}: status={status};output={expected};seams={string.Join(",", state.Seams)};tokens=same;trap=0";
}

static void Check(bool condition, string message)
{
    if (!condition)
        throw new InvalidOperationException(message);
}

sealed class State(string mode, CancellationToken expected)
{
    public string Mode { get; } = mode;
    public string Label { get; } = "borrowed-request";
    public StringBuilder Output { get; } = new();
    public List<string> Seams { get; } = [];
    public List<CancellationToken> Tokens { get; } = [];
    public TaskCompletionSource Entered { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public TaskCompletionSource Gate { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public IOException Original { get; } = new("original combined request destination failure");
    public int Flushes { get; private set; }
    public int Pending { get; private set; }
    public int Traps { get; set; }

    public void Observe(RazorSlice slice, string seam)
    {
        if (slice.CancellationToken != expected || Label != "borrowed-request")
            throw new InvalidOperationException($"Wrong request/token at {seam}.");
        Seams.Add(seam);
        Tokens.Add(slice.CancellationToken);
    }

    public void Accept(string value)
    {
        if (Mode == "write-error" && value == "foot")
        {
            Output.Append('f');
            Pending++;
            throw Original;
        }
        Output.Append(value);
        Pending += Encoding.UTF8.GetByteCount(value);
    }

    public Task Flush(CancellationToken token)
    {
        if (token != expected)
            throw new InvalidOperationException("The native flush must receive the request token.");
        Flushes++;
        Pending = 0;
        if (Mode == "flush-cancel")
        {
            Entered.SetResult();
            return Gate.Task.WaitAsync(token);
        }
        return Task.CompletedTask;
    }
}

sealed class Page : RazorSlice<State>, IUsesLayout<InnerProxy, State>
{
    public State LayoutModel => Model;
    public override async Task ExecuteAsync()
    {
        Model.Observe(this, "page-body");
        WriteLiteral("P");
        await RenderPartialAsync(new Leaf("bare-child", "b") { Model = Model });
    }

    protected internal override async Task ExecuteSectionAsync(string name)
    {
        if (name == "header")
        {
            Model.Observe(this, "page-header");
            WriteLiteral("H");
            await RenderPartialAsync(new Leaf("header-child", "h") { Model = Model });
        }
        else if (name == "page-only")
        {
            Model.Observe(this, "wrong-fallback");
            WriteLiteral("BAD");
        }
    }
}

sealed class Leaf(string seam, string value) : RazorSlice<State>, IUsesLayout<TrapProxy, State>
{
    public State LayoutModel => Model;
    public override Task ExecuteAsync()
    {
        Model.Observe(this, seam);
        WriteLiteral(value);
        return Task.CompletedTask;
    }
}

sealed class Inner : RazorLayoutSlice<State>, IUsesLayout<OuterProxy, State>
{
    public State LayoutModel => Model;
    public override async Task ExecuteAsync()
    {
        Model.Observe(this, "inner-body");
        WriteLiteral("I");
        await RenderSectionAsync("header");
        await RenderBodyAsync();
        WriteLiteral("i");
    }

    protected internal override async Task ExecuteSectionAsync(string name)
    {
        if (name == "footer")
        {
            Model.Observe(this, "inner-footer");
            WriteLiteral("F");
            await RenderSectionAsync("header");
            if (Model.Mode == "section-cancel")
            {
                Model.Entered.SetResult();
                await Model.Gate.Task.WaitAsync(CancellationToken);
            }
            await RenderPartialAsync(new Leaf("footer-child", "foot") { Model = Model });
            await FlushAsync();
        }
        else
        {
            Model.Observe(this, "inner-missing");
        }
    }
}

sealed class Outer : RazorLayoutSlice<State>
{
    public override async Task ExecuteAsync()
    {
        Model.Observe(this, "outer-body");
        WriteLiteral("O");
        await RenderBodyAsync();
        WriteLiteral("|");
        await RenderSectionAsync("footer");
        await RenderSectionAsync("page-only");
        WriteLiteral("o");
    }
}

sealed class Trap : RazorLayoutSlice<State>
{
    public override Task ExecuteAsync()
    {
        Model.Traps++;
        WriteLiteral("TRAP");
        return Task.CompletedTask;
    }
}

sealed class PartialHost : RazorSlice<State>
{
    public override async Task ExecuteAsync() => await RenderPartialAsync(new Page { Model = Model });
}
sealed class InnerProxy : IRazorSliceProxy<State>
{
    public static RazorSlice<State> CreateSlice(State state) => new Inner { Model = state };
}
sealed class OuterProxy : IRazorSliceProxy<State>
{
    public static RazorSlice<State> CreateSlice(State state) => new Outer { Model = state };
}
sealed class TrapProxy : IRazorSliceProxy<State>
{
    public static RazorSlice<State> CreateSlice(State state) => new Trap { Model = state };
}

sealed class CountingText(State state) : StringWriter
{
    public override void Write(string? value) => state.Accept(value ?? "");
    public override Task FlushAsync(CancellationToken cancellationToken) => state.Flush(cancellationToken);
}

sealed class CountingPipe(State state) : PipeWriter
{
    private readonly ArrayBufferWriter<byte> _buffer = new();
    public override bool CanGetUnflushedBytes => true;
    public override long UnflushedBytes => state.Pending;
    public override Memory<byte> GetMemory(int sizeHint = 0) => _buffer.GetMemory(sizeHint);
    public override Span<byte> GetSpan(int sizeHint = 0) => _buffer.GetSpan(sizeHint);
    public override void Advance(int bytes)
    {
        _buffer.Advance(bytes);
        state.Accept(Encoding.UTF8.GetString(_buffer.WrittenSpan[^bytes..]));
    }
    public override async ValueTask<FlushResult> FlushAsync(CancellationToken cancellationToken = default)
    {
        await state.Flush(cancellationToken);
        return new FlushResult(isCanceled: false, isCompleted: false);
    }
    public override void CancelPendingFlush() { }
    public override void Complete(Exception? exception = null) { }
}
