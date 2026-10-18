using System.Buffers;
using System.IO.Pipelines;
using System.Text;
using RazorSlices;

foreach (var (name, mode, values) in new (string, string, string[])[]
{
    ("boolean-empty", "boolean", []),
    ("boolean-values", "boolean", ["&", "<"]),
    ("binding-empty", "binding", []),
    ("binding-values", "binding", ["&", "<"]),
    ("nested", "nested", ["&", "<"])
})
{
    var textModel = new Model(values);
    using var text = new StringWriter();
    await new Loop(textModel, mode).RenderAsync(text);
    var pipeModel = new Model(values);
    var pipe = new Pipe();
    await new Loop(pipeModel, mode).RenderAsync(pipe.Writer);
    await pipe.Writer.CompleteAsync();
    var read = await pipe.Reader.ReadAsync();
    var output = Encoding.UTF8.GetString(read.Buffer.ToArray());
    pipe.Reader.AdvanceTo(read.Buffer.End);
    await pipe.Reader.CompleteAsync();
    if (output != text.ToString() ||
        textModel.Conditions != values.Length + 1 || pipeModel.Conditions != values.Length + 1 ||
        textModel.Index != values.Length || pipeModel.Index != values.Length)
        throw new InvalidOperationException("Output, body count and condition order must match.");
    Console.WriteLine($"{name}: {output}");
}

var heldModel = new Model(["&", "<"]);
using var heldOutput = new StringWriter();
var gate = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
var held = new HeldLoop(heldModel, gate.Task).RenderAsync(heldOutput);
if (held.IsCompleted || heldOutput.ToString() != "[0:" ||
    heldModel.Conditions != 1 || heldModel.Index != 1)
    throw new InvalidOperationException("Held loop must stop within its first iteration.");
gate.SetResult();
await held;
if (heldOutput.ToString() != "[0:&amp;;1:&lt;;]" || heldModel.Conditions != 3)
    throw new InvalidOperationException("Held loop must resume in order.");

const string expected = "[0:&amp;;1:&lt;;]";
for (var limit = 0; limit < expected.Length; limit++)
{
    using var failed = new FailingWriter(limit);
    var model = new Model(["&", "<"]);
    try
    {
        await new Loop(model, "binding").RenderAsync(failed);
        throw new InvalidOperationException("The original writer error must escape.");
    }
    catch (IOException error) when (ReferenceEquals(error, failed.Failure))
    {
        if (failed.Accepted.ToString() != expected[..limit])
            throw new InvalidOperationException("Accepted error prefix changed.");
        var conditions = limit switch { 0 => 0, <= 8 => 1, <= 15 => 2, _ => 3 };
        if (model.Conditions != conditions)
            throw new InvalidOperationException("Failed iteration advanced later conditions.");
    }
}

sealed class Model(string[] values)
{
    public int Index;
    public int Conditions;
    public bool Pending()
    {
        Conditions++;
        return Index < values.Length;
    }
    public string Take() => values[Index++];
    public (int Index, string Text)? Next()
    {
        Conditions++;
        return Index < values.Length ? (Index, values[Index++]) : null;
    }
}

sealed class Loop(Model model, string mode) : RazorSlice
{
    public override Task ExecuteAsync()
    {
        WriteLiteral("[");
        if (mode == "boolean")
        {
            while (model.Pending())
                Write(model.Take());
        }
        else
        {
            while (model.Next() is { } row)
            {
                Write(row.Index);
                WriteLiteral(":");
                Write(row.Text);
                if (mode == "nested")
                    for (var inner = 0; inner < 2; inner++)
                        Write(inner);
                WriteLiteral(";");
            }
        }
        WriteLiteral("]");
        return Task.CompletedTask;
    }
}

sealed class HeldLoop(Model model, Task gate) : RazorSlice
{
    public override async Task ExecuteAsync()
    {
        WriteLiteral("[");
        while (model.Next() is { } row)
        {
            Write(row.Index);
            WriteLiteral(":");
            await gate;
            Write(row.Text);
            WriteLiteral(";");
        }
        WriteLiteral("]");
    }
}

sealed class FailingWriter(int limit) : TextWriter
{
    public override Encoding Encoding => Encoding.UTF8;
    public readonly IOException Failure = new("original loop writer failure");
    public readonly StringBuilder Accepted = new();
    public override void Write(char value)
    {
        if (Accepted.Length == limit)
            throw Failure;
        Accepted.Append(value);
    }
}
