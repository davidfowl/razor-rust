using System.Buffers;
using System.IO.Pipelines;
using System.Reflection;
using System.Text;
using Microsoft.AspNetCore.Razor.Hosting;
using RazorSlices;

var item = new RazorCompiledItemLoader().LoadItems(Assembly.GetExecutingAssembly()).Single();
SwitchSlice Create(Effects effects)
{
    var slice = (SwitchSlice)Activator.CreateInstance(item.Type)!;
    slice.Effects = effects;
    return slice;
}

foreach (var value in new[] { -1, 0, 1, 2, 3, 9 })
{
    var textEffects = new Effects(value);
    var pipeEffects = new Effects(value);
    var textSlice = Create(textEffects);
    var pipeSlice = Create(pipeEffects);
    string? first = null;
    for (var attempt = 0; attempt < 2; attempt++)
    {
        textEffects.Trace.Clear();
        pipeEffects.Trace.Clear();
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
            !textEffects.Trace.SequenceEqual(pipeEffects.Trace))
            throw new InvalidOperationException("Selected output and effects must repeat.");
        first = output;
    }
    Console.WriteLine($"{value}: {Escape(first!)}|{string.Join(',', textEffects.Trace)}");
}

const string expected = "A<u>guard:3</u>Z\n";
for (var limit = 0; limit < expected.Length; limit++)
{
    var effects = new Effects(3);
    using var writer = new FailingWriter(limit);
    try
    {
        await Create(effects).RenderAsync(writer);
        throw new InvalidOperationException("The original writer failure must escape.");
    }
    catch (IOException error) when (ReferenceEquals(error, writer.Failure))
    {
        if (writer.Accepted.ToString() != expected[..limit])
            throw new InvalidOperationException("Accepted output must be the exact prefix.");
        Console.WriteLine($"fail-{limit}: {Escape(writer.Accepted.ToString())}|{string.Join(',', effects.Trace)}");
    }
}

static string Escape(string text) => text.Replace("\r", "\\r").Replace("\n", "\\n");

public abstract class SwitchSlice : RazorSlice
{
    public Effects Effects { get; set; } = null!;
}

public sealed class Effects(int value)
{
    public int Value => value;
    public List<string> Trace { get; } = [];
    public int Select() { Trace.Add("select"); return Value; }
    public bool Guard(int candidate) { Trace.Add("guard"); return candidate == 3; }
    public string Touch(string label) { Trace.Add(label); return label; }
}

sealed class FailingWriter(int limit) : TextWriter
{
    public override Encoding Encoding => Encoding.UTF8;
    public readonly IOException Failure = new("original match writer failure");
    public readonly StringBuilder Accepted = new();
    public override void Write(char value)
    {
        if (Accepted.Length == limit)
            throw Failure;
        Accepted.Append(value);
    }
}
