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

    public static async Task RunCharacters()
    {
        foreach (var (name, value) in new[]
        {
            ("letter", 'A'), ("digit", '7'), ("space", ' '), ("ampersand", '&'),
            ("less-than", '<'), ("greater-than", '>'), ("double-quote", '"'),
            ("dash", '-'), ("underscore", '_')
        })
            await Case(name, value);
        await Case<char?>("nullable-none", null);
        await Case<char?>("nullable-some", 'Z');
        await CaseSlice("attribute-scalar", () => new CharacterAttribute('&'));
        await CaseSlice("attribute-none", () => new CharacterAttribute(null));
        await CaseSlice("attribute-some", () => new CharacterAttribute('<'));
    }

    public static async Task RunPrimitiveOutcomes()
    {
        CultureInfo.CurrentCulture = CultureInfo.InvariantCulture;
        await Outcome("bool-direct-true", () => new Boolean(true));
        await Outcome("bool-direct-false", () => new Boolean(false));
        await Outcome("bool-generic-true", () => new Value<bool>(true));
        await Outcome("bool-generic-false", () => new Value<bool>(false));
        await Outcome("double-nan", () => new Value<double>(double.NaN));
        await Outcome("double-positive-infinity", () => new Value<double>(double.PositiveInfinity));
        await Outcome("double-negative-infinity", () => new Value<double>(double.NegativeInfinity));
        await Outcome("double-negative-zero", () => new Value<double>(-0.0));
        await Outcome("double-small-exponent", () => new Value<double>(1e-30));
        await Outcome("double-large-exponent", () => new Value<double>(1e30));
        await Outcome("single-nan", () => new Value<float>(float.NaN));
        await Outcome("single-positive-infinity", () => new Value<float>(float.PositiveInfinity));
        await Outcome("single-negative-infinity", () => new Value<float>(float.NegativeInfinity));
        await Outcome("single-negative-zero", () => new Value<float>(-0.0f));
        await Outcome("character-single-quote", () => new Value<char>('\''));
        await Outcome("character-double-quote", () => new Value<char>('"'));
        await Outcome("character-unicode", () => new Value<char>('\u96ea'));
        await Outcome("rune-supplementary", () => new Value<Rune>(new(0x1F980)));
    }

    private static Task Case<T>(string name, T value)
        => CaseSlice(name, () => new Value<T>(value));

    private static async Task CaseSlice(string name, Func<RazorSlice> create)
    {
        var (pipe, text) = await Capture(create);
        if (pipe != text)
            throw new InvalidOperationException("PipeWriter/TextWriter output must match.");
        Console.WriteLine($"{name}: {text}");
    }

    private static async Task Outcome(string name, Func<RazorSlice> create)
    {
        var (pipe, text) = await Capture(create);
        Console.WriteLine($"{name}: pipe={pipe},text={text}");
    }

    private static async Task<(string Pipe, string Text)> Capture(Func<RazorSlice> create)
    {
        using var text = new StringWriter(CultureInfo.InvariantCulture);
        await create().RenderAsync(text);
        var pipe = new Pipe();
        await create().RenderAsync(pipe.Writer);
        await pipe.Writer.CompleteAsync();
        var read = await pipe.Reader.ReadAsync();
        var bytes = read.Buffer.ToArray();
        pipe.Reader.AdvanceTo(read.Buffer.End);
        await pipe.Reader.CompleteAsync();
        return (Encoding.UTF8.GetString(bytes), text.ToString());
    }

    private sealed class Boolean(bool value) : RazorSlice
    {
        public override Task ExecuteAsync()
        {
            WriteLiteral("[");
            Write((bool?)value);
            WriteLiteral("]");
            return Task.CompletedTask;
        }
    }

    private sealed class CharacterAttribute(char? value) : RazorSlice
    {
        public override Task ExecuteAsync()
        {
            WriteLiteral("[");
            BeginWriteAttribute("data-char", " data-char=\"", 0, "\"", 0, 1);
            WriteAttributeValue("", 0, value, 0, 1, false);
            EndWriteAttribute();
            WriteLiteral("]");
            return Task.CompletedTask;
        }
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
