using System.Buffers;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;

foreach (var (name, bytes) in new (string, byte[])[]
{
    ("empty", []),
    ("ascii", "Hello 123"u8.ToArray()),
    ("escaped", "A&<B>\""u8.ToArray()),
    ("nul", [0]),
    ("unicode", [0xE9, 0x9B, 0xAA, 0xF0, 0x9F, 0xA6, 0x80]),
    ("invalid-leading", [0xFF, 0x26]),
    ("invalid-continuation", [0xE2, 0x28, 0xA1]),
    ("truncated", [0xF0, 0x9F, 0xA6]),
    ("overlong", [0xC0, 0xAF]),
    ("surrogate", [0xED, 0xA0, 0x80]),
    ("beyond-unicode", [0xF4, 0x90, 0x80, 0x80])
})
{
    if (args is ["common"] && name is not ("empty" or "ascii" or "escaped"))
        continue;
    var expected = await Capture(() => new TextValue(Encoding.UTF8.GetString(bytes)));
    foreach (var mode in new[] { "array", "memory", "span" })
    {
        var actual = await Capture(() => new ByteValue(bytes, mode));
        if (actual != expected)
            throw new InvalidOperationException($"{name}/{mode}: bytes must match replacement-decoded string output.");
    }
    Console.WriteLine($"{name}: {expected}");
}

static async Task<string> Capture(Func<RazorSlice> create)
{
    using var text = new StringWriter();
    await create().RenderAsync(text);
    var pipe = new Pipe();
    await create().RenderAsync(pipe.Writer);
    await pipe.Writer.CompleteAsync();
    var read = await pipe.Reader.ReadAsync();
    var output = Encoding.UTF8.GetString(read.Buffer.ToArray());
    pipe.Reader.AdvanceTo(read.Buffer.End);
    await pipe.Reader.CompleteAsync();
    if (output != text.ToString())
        throw new InvalidOperationException("PipeWriter and TextWriter output must agree.");
    return output;
}

sealed class ByteValue(byte[] bytes, string mode) : RazorSlice
{
    public override Task ExecuteAsync()
    {
        WriteLiteral("[");
        if (mode == "array")
            Write(bytes);
        else if (mode == "memory")
            Write((ReadOnlyMemory<byte>)bytes);
        else
            Write(bytes.AsSpan());
        WriteLiteral("]");
        return Task.CompletedTask;
    }
}

sealed class TextValue(string value) : RazorSlice
{
    public override Task ExecuteAsync()
    {
        WriteLiteral("[");
        Write(value);
        WriteLiteral("]");
        return Task.CompletedTask;
    }
}
