using System.Buffers;
using System.Globalization;
using System.IO.Pipelines;
using System.Text;
using Microsoft.AspNetCore.Html;
using RazorSlices;

static class OptionalValueCases
{
    public static async Task Run()
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        await Case<string?>("string-null", null);
        await Case<string?>("string-empty", "");
        await Case<string?>("string-encoded", "A&<>Z");
        await Case<int?>("integer-null", null);
        await Case<int?>("integer-zero", 0);
        await Case<int?>("integer-negative", -42);
        await Case<long?>("integer-min", long.MinValue);
        await Case<long?>("integer-max", long.MaxValue);
        await Case<HtmlString?>("html-null", null);
        await Case<HtmlString?>("html-trusted", new("<b>&\"'\u96ea</b>"));
        await Case<double?>("float-null", null);
        await Case<double?>("float-value", 1.5);
    }

    private static async Task Case<T>(string name, T value)
    {
        using var text = new StringWriter(CultureInfo.InvariantCulture);
        await new Value<T>(value).RenderAsync(text);
        var pipe = new Pipe();
        await new Value<T>(value).RenderAsync(pipe.Writer);
        await pipe.Writer.CompleteAsync();
        var read = await pipe.Reader.ReadAsync();
        var bytes = read.Buffer.ToArray();
        pipe.Reader.AdvanceTo(read.Buffer.End);
        await pipe.Reader.CompleteAsync();
        if (Encoding.UTF8.GetString(bytes) != text.ToString())
            throw new InvalidOperationException("Nullable PipeWriter/TextWriter output must match.");
        Console.WriteLine($"{name}: {text}");
    }

    private sealed class Value<T>(T value) : RazorSlice
    {
        public override Task ExecuteAsync()
        {
            WriteLiteral("[");
            Write(value);
            WriteLiteral("]");
            return Task.CompletedTask;
        }
    }
}
