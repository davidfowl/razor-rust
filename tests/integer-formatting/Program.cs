using System.Buffers;
using System.Globalization;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;

foreach (var value in new ulong[] { 0, 1, 15, 16, 42, 255, 256, ulong.MaxValue })
foreach (var format in new[] { "D", "x", "X" })
foreach (var precision in new[] { 0, 1, 4, 20, 32, 255 })
    await Emit(value, format, precision, -1);
foreach (var precision in new[] { 4, 255 })
foreach (var failAt in precision == 4 ? new[] { 0, 1, 2, 3, 4, 5 } : new[] { 0, 1, 2, 128, 255, 256 })
    await Emit(255, "X", precision, failAt);

static async Task Emit(ulong value, string format, int precision, int failAt)
{
    var text = await Render(false, value, format, precision, failAt);
    var pipe = await Render(true, value, format, precision, failAt);
    if (text != pipe) throw new InvalidOperationException("Text/Pipe mismatch.");
    var label = failAt < 0 ? $"{value}/{format}{precision}" : $"error/{precision}/{failAt}";
    Console.WriteLine($"common/{label}: {text}");
}

static async Task<string> Render(bool pipe, ulong value, string format, int precision, int failAt)
{
    var destination = new Destination(pipe, failAt);
    var page = new Page(value, format + precision);
    var status = "ok";
    try
    {
        if (pipe) await page.RenderAsync(destination.Pipe!);
        else await page.RenderAsync(destination.Text!);
    }
    catch (IOException error) when (ReferenceEquals(error, destination.Failure))
    {
        status = "same-error";
    }
    var expected = "P" + value.ToString(format + precision, CultureInfo.InvariantCulture) + "S";
    if (failAt >= 0) expected = expected[..failAt];
    if (destination.Output != expected || status != (failAt < 0 ? "ok" : "same-error"))
        throw new InvalidOperationException($"Exact output/error prefix mismatch: pipe={pipe}, value={value}, format={format}{precision}, fail={failAt}, status={status}, bytes={destination.Output.Length}, expected={expected.Length}.");
    if (destination.Ends != 0) throw new InvalidOperationException("Rendering must not own destination.");
    destination.End();
    if (destination.Ends != 1) throw new InvalidOperationException("Caller must retain destination.");
    ulong hash = 14695981039346656037;
    foreach (var b in Encoding.UTF8.GetBytes(destination.Output))
        hash = unchecked((hash ^ b) * 1099511628211);
    return $"status={status};bytes={destination.Output.Length};fnv1a={hash:x16}";
}

sealed class Page(ulong value, string format) : RazorSlice
{
    public override Task ExecuteAsync()
    {
        WriteLiteral("P");
        WriteNumber((ulong?)value, format, CultureInfo.InvariantCulture);
        WriteLiteral("S");
        return Task.CompletedTask;
    }
}

sealed class Destination
{
    public readonly IOException Failure = new("original integer destination failure");
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
    public int Accepted(int count, int written)
        => FailAt < 0 ? count : Math.Min(count, Math.Max(0, FailAt - written));
    public void End()
    {
        if (Pipe is { } pipe) pipe.Complete();
        else Text!.Dispose();
    }
}

sealed class Text(Destination destination) : StringWriter
{
    public override void Write(string? value) => Write(value.AsSpan());
    public override void Write(char[] buffer, int index, int count) => Write(buffer.AsSpan(index, count));
    public override void Write(ReadOnlySpan<char> value)
    {
        var accepted = destination.Accepted(value.Length, GetStringBuilder().Length);
        GetStringBuilder().Append(value[..accepted]);
        if (accepted != value.Length) throw destination.Failure;
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
    public override void Advance(int bytes)
    {
        var accepted = destination.Accepted(bytes, Buffer.WrittenCount);
        Buffer.Advance(accepted);
        if (accepted != bytes) throw destination.Failure;
    }
    public override Memory<byte> GetMemory(int sizeHint = 0) => Buffer.GetMemory(sizeHint);
    public override Span<byte> GetSpan(int sizeHint = 0) => Buffer.GetSpan(sizeHint);
    public override ValueTask<FlushResult> FlushAsync(CancellationToken cancellationToken = default)
        => ValueTask.FromResult(new FlushResult(false, false));
    public override void CancelPendingFlush() => throw new NotSupportedException();
    public override void Complete(Exception? exception = null) => destination.Ends++;
}
