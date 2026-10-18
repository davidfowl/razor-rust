using System.Buffers;
using System.Globalization;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;

foreach (var kind in new[] { "decimal", "negative", "hex", "fixed", "custom", "empty", "markup", "long", "reuse" })
    await Emit(kind, -1);
for (var failAt = 0; failAt < Encoding.UTF8.GetByteCount("P&lt;custom&amp;&gt;S"); failAt++)
    await Emit("failure", failAt);

using var cultureOutput = new StringWriter();
await new Probe("culture", new Provider()).RenderAsync(cultureOutput);
Console.WriteLine($"csharp/provider-fr: output={cultureOutput}");
Console.WriteLine("csharp/custom-formatter: calls=0;explicit-format-and-provider-forwarded=true");

static async Task Emit(string kind, int failAt)
{
    var text = await Run(false, kind, failAt);
    var pipe = await Run(true, kind, failAt);
    if (text != pipe) throw new InvalidOperationException($"Text/Pipe mismatch: {kind}/{failAt}");
    Console.WriteLine($"common/{(failAt < 0 ? kind : $"error-{failAt}")}: {text}");
}

static async Task<string> Run(bool pipe, string kind, int failAt)
{
    var destination = new Destination(pipe, failAt);
    var provider = new Provider();
    var page = new Probe(kind, provider);
    var status = "ok";
    try
    {
        for (var round = 0; round < (kind == "reuse" ? 2 : 1); round++)
        {
            if (pipe) await page.RenderAsync(destination.Pipe!);
            else await page.RenderAsync(destination.Text!);
        }
    }
    catch (IOException error) when (ReferenceEquals(error, destination.Failure))
    {
        status = "same-error";
    }
    if (provider.Calls != 0) throw new InvalidOperationException("Span helpers must not dispatch ICustomFormatter.");
    if (destination.Ends != 0) throw new InvalidOperationException("Render took destination ownership.");
    destination.End();
    if (destination.Ends != 1) throw new InvalidOperationException("Caller must retain destination ownership.");
    var bytes = Encoding.UTF8.GetBytes(destination.Output);
    if (kind != "long") return $"status={status};output={destination.Output}";
    if (destination.Output != new string('x', 8192) + "&lt;&amp;&gt;")
        throw new InvalidOperationException("Long formatted output must match exactly.");
    ulong hash = 14695981039346656037;
    foreach (var value in bytes) hash = unchecked((hash ^ value) * 1099511628211);
    return $"status={status};bytes={bytes.Length};fnv1a={hash:x16}";
}

sealed class Probe(string kind, Provider provider) : RazorSlice
{
    public override Task ExecuteAsync()
    {
        switch (kind)
        {
            case "decimal":
            case "reuse": WriteNumber((int?)42, "D4", CultureInfo.InvariantCulture); break;
            case "negative": WriteNumber((int?)-42, "D4", CultureInfo.InvariantCulture); break;
            case "hex": WriteNumber((int?)255, "X4", CultureInfo.InvariantCulture); break;
            case "fixed": WriteNumber(12.5, "F2", CultureInfo.InvariantCulture); break;
            case "culture": WriteNumber(12.5, "F2", CultureInfo.GetCultureInfo("fr-FR")); break;
            case "custom":
            case "empty":
            case "markup":
            case "long": WriteSpanFormattable(new Custom(kind, provider), "tag", provider); break;
            case "failure":
                WriteLiteral("P");
                WriteSpanFormattable(new Custom("custom", provider), "tag", provider);
                WriteLiteral("S");
                break;
            default: throw new InvalidOperationException($"Unknown case {kind}.");
        }
        return Task.CompletedTask;
    }
}

sealed class Custom(string kind, Provider expectedProvider) : ISpanFormattable
{
    private static readonly string Long = new string('x', 8192) + "<&>";
    public bool TryFormat(Span<char> destination, out int charsWritten, ReadOnlySpan<char> format,
        IFormatProvider? provider)
    {
        if (!format.SequenceEqual("tag") || !ReferenceEquals(provider, expectedProvider))
            throw new InvalidOperationException("Explicit format/provider was not forwarded.");
        var text = kind switch
        {
            "custom" => "<custom&>",
            "empty" => "",
            "markup" => "<b>",
            "long" => Long,
            _ => throw new InvalidOperationException($"Unknown custom case {kind}."),
        };
        charsWritten = text.Length;
        return text.AsSpan().TryCopyTo(destination);
    }
    public string ToString(string? format, IFormatProvider? provider) => throw new NotSupportedException();
}

sealed class Provider : IFormatProvider, ICustomFormatter
{
    public int Calls;
    public object? GetFormat(Type? type) => type == typeof(ICustomFormatter) ? this : null;
    public string Format(string? format, object? value, IFormatProvider? provider)
    {
        Calls++;
        return "CUSTOM-FORMATTER";
    }
}

sealed class Destination
{
    public readonly IOException Failure = new("original formatted value destination failure");
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
