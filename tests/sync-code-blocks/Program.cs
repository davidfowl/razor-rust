using System.Buffers;
using System.IO.Pipelines;
using System.Reflection;
using System.Text;
using Microsoft.AspNetCore.Razor.Hosting;
using RazorSlices;

var item = new RazorCompiledItemLoader().LoadItems(Assembly.GetExecutingAssembly()).Single();
LocalSlice Create(Effects effects)
{
    var slice = (LocalSlice)Activator.CreateInstance(item.Type)!;
    slice.Effects = effects;
    return slice;
}

foreach (var (name, prefix, values) in new (string, string, int[])[]
{
    ("empty", "", []),
    ("multiple", "&", [1, 2]),
    ("single", "<", [7])
})
{
    var textEffects = new Effects(prefix, values);
    var textSlice = Create(textEffects);
    var pipeEffects = new Effects(prefix, values);
    var pipeSlice = Create(pipeEffects);
    string? first = null;
    for (var attempt = 1; attempt <= 2; attempt++)
    {
        using var text = new StringWriter();
        await textSlice.RenderAsync(text);
        var pipe = new Pipe();
        await pipeSlice.RenderAsync(pipe.Writer);
        await pipe.Writer.CompleteAsync();
        var read = await pipe.Reader.ReadAsync();
        var output = Encoding.UTF8.GetString(read.Buffer.ToArray());
        pipe.Reader.AdvanceTo(read.Buffer.End);
        await pipe.Reader.CompleteAsync();
        if (output != text.ToString() || (first is not null && first != output) ||
            textEffects.Touches != attempt * (values.Length + 1) ||
            pipeEffects.Touches != textEffects.Touches ||
            textEffects.Exits != attempt || pipeEffects.Exits != attempt)
            throw new InvalidOperationException("Local state, output and cleanup must repeat.");
        first = output;
    }
    Console.WriteLine($"{name}: {Escape(first!)}");
}

const string expected = "[&amp;<span>1</span><span>2</span>3]\n";
for (var limit = 0; limit < expected.Length; limit++)
{
    var effects = new Effects("&", [1, 2]);
    using var writer = new FailingWriter(limit);
    try
    {
        await Create(effects).RenderAsync(writer);
        throw new InvalidOperationException("The original writer failure must escape.");
    }
    catch (IOException error) when (ReferenceEquals(error, writer.Failure))
    {
        if (writer.Accepted.ToString() != expected[..limit] || effects.Exits != 1)
            throw new InvalidOperationException("Original prefix and local cleanup must survive.");
        Console.WriteLine($"fail-{limit}: {Escape(writer.Accepted.ToString())}|touches={effects.Touches},exits={effects.Exits}");
    }
}

static string Escape(string text) => text.Replace("\r", "\\r").Replace("\n", "\\n");

public abstract class LocalSlice : RazorSlice
{
    public Effects Effects { get; set; } = null!;
}

public sealed class Effects(string prefix, int[] values)
{
    public string Prefix => prefix;
    public int[] Values => values;
    public int Touches;
    public int Exits;
    public void Touch() => Touches++;
    public IDisposable Enter() => new Lease(this);
    private sealed class Lease(Effects effects) : IDisposable
    {
        public void Dispose() => effects.Exits++;
    }
}

sealed class FailingWriter(int limit) : TextWriter
{
    public override Encoding Encoding => Encoding.UTF8;
    public readonly IOException Failure = new("original code-block writer failure");
    public readonly StringBuilder Accepted = new();
    public override void Write(char value)
    {
        if (Accepted.Length == limit)
            throw Failure;
        Accepted.Append(value);
    }
}
