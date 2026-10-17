using RazorSlices;

if (args is ["native-accounting"])
{
    await NativeSinkCases.Run();
    return;
}

using var document = new StringWriter();
await new Page().RenderAsync(document);
Console.WriteLine($"document: {document}");
using var partial = new StringWriter();
await new PartialHost().RenderAsync(partial);
Console.WriteLine($"partial: {partial}");
Console.WriteLine($"section-renders: header={Counts.Header},footer={Counts.Footer},banner={Counts.Banner}");
try
{
    await new EmptyName().RenderAsync(new StringWriter());
    throw new InvalidOperationException("Empty section names must fail.");
}
catch (ArgumentException)
{
    Console.WriteLine("empty-name: rejected");
}

static class Counts
{
    public static int Header;
    public static int Footer;
    public static int Banner;
}

sealed class Page : RazorSlice, IUsesLayout<InnerProxy>
{
    public override Task ExecuteAsync()
    {
        WriteLiteral("page");
        return Task.CompletedTask;
    }

    protected internal override Task ExecuteSectionAsync(string name)
    {
        if (name == "header")
        {
            Counts.Header++;
            WriteLiteral("page-header");
        }
        else if (name == "page-only")
        {
            WriteLiteral("must-not-inherit");
        }
        return Task.CompletedTask;
    }
}

sealed class Inner : RazorLayoutSlice, IUsesLayout<OuterProxy>
{
    public override async Task ExecuteAsync()
    {
        WriteLiteral("<inner>");
        await RenderSectionAsync("header");
        WriteLiteral(":");
        await RenderBodyAsync();
        WriteLiteral("</inner>");
    }

    protected internal override async Task ExecuteSectionAsync(string name)
    {
        if (name == "footer")
        {
            Counts.Footer++;
            WriteLiteral("inner-footer[");
            await RenderSectionAsync("header");
            WriteLiteral("]");
        }
    }
}

sealed class Outer : RazorLayoutSlice, IUsesLayout<TopProxy>
{
    public override async Task ExecuteAsync()
    {
        WriteLiteral("<outer>");
        await RenderBodyAsync();
        WriteLiteral("|");
        await RenderSectionAsync("footer");
        WriteLiteral("|");
        await RenderSectionAsync("footer");
        await RenderSectionAsync("page-only");
        await RenderSectionAsync("Footer");
        await RenderSectionAsync(" ");
        WriteLiteral("</outer>");
    }

    protected internal override async Task ExecuteSectionAsync(string name)
    {
        if (name == "banner")
        {
            Counts.Banner++;
            WriteLiteral("outer-banner{");
            await RenderSectionAsync("footer");
            WriteLiteral("}");
        }
    }
}

sealed class Top : RazorLayoutSlice
{
    public override async Task ExecuteAsync()
    {
        WriteLiteral("<top>");
        await RenderBodyAsync();
        WriteLiteral("|");
        await RenderSectionAsync("banner");
        WriteLiteral("</top>");
    }
}

sealed class PartialHost : RazorSlice
{
    public override async Task ExecuteAsync() => await RenderPartialAsync(new Page());
}

sealed class EmptyName : RazorLayoutSlice
{
    public override async Task ExecuteAsync() => await RenderSectionAsync("");
}

sealed class InnerProxy : IRazorSliceProxy
{
    public static RazorSlice CreateSlice() => new Inner();
}

sealed class OuterProxy : IRazorSliceProxy
{
    public static RazorSlice CreateSlice() => new Outer();
}

sealed class TopProxy : IRazorSliceProxy
{
    public static RazorSlice CreateSlice() => new Top();
}
