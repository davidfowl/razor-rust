using System.Diagnostics;
using System.Globalization;
using System.Linq.Expressions;
using System.Reflection;
using System.Text;
using System.Text.Json;
using Microsoft.AspNetCore.Razor.Hosting;
using RazorSlices;

if (args.Length != 6 || args[0] is not ("verify" or "measure"))
    throw new ArgumentException("usage: RenderComparison verify|measure FIXTURE EXPECTED WARMUP SAMPLES ITERATIONS");
#if DEBUG
throw new InvalidOperationException("Build Release; debug measurements are forbidden.");
#endif
var jsonOptions = new JsonSerializerOptions { PropertyNameCaseInsensitive = true };
var fixture = JsonSerializer.Deserialize<Fixture>(File.ReadAllBytes(args[1]), jsonOptions)
    ?? throw new InvalidOperationException("Null fixture.");
if (fixture.Repeat is < 1 or > 1024 || fixture.Items.Length == 0)
    throw new InvalidOperationException("Fixture must have items and repeat in 1..=1024.");
var data = new ListData(fixture.Title,
    Enumerable.Range(0, fixture.Repeat).SelectMany(_ => fixture.Items).ToArray());
foreach (var text in data.Items.SelectMany(item => new[] { item.Name, item.Detail }).Prepend(data.Title))
    if (text.Any(character => !char.IsAsciiLetterOrDigit(character) && !" &<>".Contains(character)))
        throw new InvalidOperationException("Fixture exceeds common encoder subset.");
var expected = File.ReadAllBytes(args[2]);
var warmup = int.Parse(args[3], CultureInfo.InvariantCulture);
var samples = int.Parse(args[4], CultureInfo.InvariantCulture);
var iterations = int.Parse(args[5], CultureInfo.InvariantCulture);
if (warmup <= 0 || samples <= 0 || iterations <= 0)
    throw new ArgumentException("Protocol counts must be positive.");
var compiledItem = new RazorCompiledItemLoader().LoadItems(Assembly.GetExecutingAssembly()).Single();
var create = Expression.Lambda<Func<ListSlice>>(Expression.New(compiledItem.Type)).Compile();

string Render()
{
    var slice = create();
    slice.Data = data;
    using var writer = new StringWriter(CultureInfo.InvariantCulture);
    var result = slice.RenderAsync(writer);
    if (!result.IsCompleted)
        throw new InvalidOperationException("This synchronous workload must complete on the measuring thread.");
    result.GetAwaiter().GetResult();
    return writer.ToString();
}

for (var attempt = 1; attempt <= 2; attempt++)
{
    var actual = Encoding.UTF8.GetBytes(Render());
    if (!actual.AsSpan().SequenceEqual(expected))
        throw new InvalidOperationException($"output mismatch: razorslices-csharp, attempt {attempt}, expected {expected.Length} bytes, actual {actual.Length} bytes; no measurements performed");
}
Console.WriteLine(JsonSerializer.Serialize(new
{
    kind = "gate", engine = "razorslices-csharp", equal = true, output_bytes = expected.Length
}));
if (args[0] == "verify")
    return;
for (var index = 0; index < warmup; index++)
    GC.KeepAlive(Render());
for (var sample = 0; sample < samples; sample++)
{
    var start = Stopwatch.GetTimestamp();
    for (var index = 0; index < iterations; index++)
        GC.KeepAlive(Render());
    var elapsed = Stopwatch.GetElapsedTime(start);
    var allocatedStart = GC.GetAllocatedBytesForCurrentThread();
    for (var index = 0; index < iterations; index++)
        GC.KeepAlive(Render());
    var allocated = GC.GetAllocatedBytesForCurrentThread() - allocatedStart;
    Console.WriteLine(JsonSerializer.Serialize(new
    {
        kind = "sample", engine = "razorslices-csharp", sample, iterations,
        elapsed_ns = (long)(elapsed.TotalSeconds * 1_000_000_000),
        renders_per_second = iterations / elapsed.TotalSeconds, output_bytes = expected.Length,
        allocation_events = (long?)null, allocation_events_per_render = (double?)null,
        allocated_bytes = allocated, allocated_bytes_per_render = (double)allocated / iterations
    }));
}

public sealed record Item(string Name, string Detail);
public sealed record Fixture(string Title, int Repeat, Item[] Items);
public sealed record ListData(string Title, Item[] Items);
public abstract class ListSlice : RazorSlice
{
    public ListData Data { get; set; } = null!;
}
