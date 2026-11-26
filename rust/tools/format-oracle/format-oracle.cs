// Generates the number and date formats of the .NET oracle: doubles and DateTimeOffset as
// RecordJson writes them (same options as src/Sermofur.Application/Contracts.cs), Math.Round to
// 6 digits as RecallService uses it, the "O" format of ChallengeService, the fixed formats of
// DaemonLog, DaemonServer, InstanceRegistry and InstanceMigration, and what RecordJson accepts
// when it reads a DateTimeOffset. Usage: dotnet run format-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
using System.Globalization;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using System.Text.Unicode;

JsonSerializerOptions options = new()
{
    PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    WriteIndented = true,
    Encoder = JavaScriptEncoder.Create(UnicodeRanges.All),
};
options.Converters.Add(new JsonStringEnumConverter(JsonNamingPolicy.SnakeCaseLower, false));

// Doubles: the bit pattern (exact input) and the text RecordJson writes.
List<double> doubles =
[
    0.0,
    -0.0,
    1.0,
    -1.0,
    2.0,
    0.5,
    0.1,
    0.2,
    0.3,
    1.1,
    1.2,
    1.0 / 3.0,
    2.0 / 3.0,
    0.1 + 0.2,
    1e-4,
    1e-5,
    1.5e-5,
    1e-6,
    1.5e-6,
    1e-7,
    1.234567e-7,
    123456.789,
    1e14,
    1e15,
    1.5e15,
    1e16,
    1e17,
    123456789012345678.0,
    1e21,
    1e22,
    1e100,
    1e-100,
    5e-324,
    double.Epsilon * 3,
    double.MaxValue,
    double.MinValue,
    9007199254740993.0,
    0.000123,
    12.5,
    4.35,
    3.14159265358979,
];

// Every power of two: the shortest form takes the upper neighbour when the gap below is half.
for (int exponent = -1074; exponent <= 1023; exponent++)
{
    doubles.Add(Math.ScaleB(1.0, exponent));
}

// Non-finite values: System.Text.Json refuses to write them (recorded as an error).
doubles.Add(double.NaN);
doubles.Add(double.PositiveInfinity);
doubles.Add(double.NegativeInfinity);
Random random = new(20261106);
for (int i = 0; i < 300; i++)
{
    doubles.Add(random.NextDouble() * Math.Pow(10, random.Next(-9, 18)));
}
for (int i = 0; i < 200; i++)
{
    doubles.Add(Math.Round(random.NextDouble() * Math.Pow(10, random.Next(-3, 4)), 6));
}
JsonArray doubleRows = [];
foreach (double value in doubles)
{
    string bits = BitConverter.DoubleToInt64Bits(value).ToString(CultureInfo.InvariantCulture);
    try
    {
        doubleRows.Add(new JsonArray(bits, JsonSerializer.Serialize(value, options)));
    }
    catch (ArgumentException)
    {
        doubleRows.Add(new JsonArray(bits, null));
    }
}

// Math.Round(value, 6), midpoint to even by default, as RecallService.cs:311, 330 and 344 call it.
List<double> roundInputs =
[
    0.0000025,
    0.0000035,
    0.1234565,
    0.1234575,
    1.0000005,
    2.5e-7,
    3.5e-7,
    0.0000004999999,
    123.4567895,
    0.9999995,
    1.2345675,
    -0.0000025,
    12345678.1234565,
    1e16 + 2,
    0.3333335,
];
for (int i = 0; i < 300; i++)
{
    roundInputs.Add(random.NextDouble() * Math.Pow(10, random.Next(-8, 9)));
}
for (int i = 0; i < 100; i++)
{
    // Values close to a midpoint at the 7th decimal.
    roundInputs.Add((random.Next(0, 10_000_000) + 0.5) / 1e6);
}
JsonArray roundRows = [];
foreach (double value in roundInputs)
{
    roundRows.Add(
        new JsonArray(
            BitConverter.DoubleToInt64Bits(value).ToString(CultureInfo.InvariantCulture),
            BitConverter
                .DoubleToInt64Bits(Math.Round(value, 6))
                .ToString(CultureInfo.InvariantCulture)
        )
    );
}

// DateTimeOffset: UTC ticks and offset in minutes, then every format the code writes.
List<DateTimeOffset> dates =
[
    new(2026, 11, 6, 11, 0, 0, TimeSpan.Zero),
    new(2026, 11, 6, 12, 0, 0, TimeSpan.FromHours(1)),
    new(2026, 7, 1, 2, 3, 4, TimeSpan.FromHours(2)),
    new(2026, 1, 1, 0, 0, 0, TimeSpan.FromMinutes(-330)),
    new(2026, 1, 1, 0, 0, 0, TimeSpan.FromHours(14)),
    new(2026, 1, 1, 0, 0, 0, TimeSpan.FromHours(-12)),
    new(1, 1, 1, 0, 0, 0, TimeSpan.Zero),
    new(9999, 12, 31, 23, 59, 59, TimeSpan.Zero),
    new(2024, 2, 29, 23, 59, 59, TimeSpan.Zero),
    new(2000, 2, 29, 12, 0, 0, TimeSpan.Zero),
    new(1900, 3, 1, 0, 0, 0, TimeSpan.Zero),
    new(1970, 1, 1, 0, 0, 0, TimeSpan.Zero),
];
long[] fractions =
[
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    1_234_567,
    1_200_000,
    9_999_999,
    5_000,
];
foreach (long fraction in fractions)
{
    dates.Add(new DateTimeOffset(2026, 11, 6, 11, 22, 33, TimeSpan.Zero).AddTicks(fraction));
    dates.Add(new DateTimeOffset(2026, 3, 29, 1, 59, 59, TimeSpan.FromHours(1)).AddTicks(fraction));
}
for (int i = 0; i < 200; i++)
{
    long ticks = (long)(random.NextDouble() * (DateTime.MaxValue.Ticks - TimeSpan.TicksPerDay));
    TimeSpan offset = TimeSpan.FromMinutes(random.Next(-14 * 4, 14 * 4 + 1) * 15);
    long local = ticks + offset.Ticks;
    if (local < 0 || local > DateTime.MaxValue.Ticks)
    {
        continue;
    }
    dates.Add(new DateTimeOffset(local, offset));
}
JsonArray dateRows = [];
foreach (DateTimeOffset date in dates)
{
    dateRows.Add(
        new JsonObject
        {
            ["utcTicks"] = date.UtcTicks.ToString(CultureInfo.InvariantCulture),
            ["offsetMinutes"] = (int)date.Offset.TotalMinutes,
            ["json"] = JsonSerializer.Serialize(date, options),
            ["o"] = date.ToString("O", CultureInfo.InvariantCulture),
            ["log"] = date.ToString("yyyy-MM-dd'T'HH:mm:ss.fffzzz", CultureInfo.InvariantCulture),
            ["seconds"] = date.ToString("yyyy-MM-dd'T'HH:mm:sszzz", CultureInfo.InvariantCulture),
            ["backup"] = date.ToUniversalTime()
                .ToString("yyyyMMdd'T'HHmmss'Z'", CultureInfo.InvariantCulture),
        }
    );
}

// Reading: what RecordJson accepts as a DateTimeOffset, and the value it gives.
List<string> parseInputs =
[
    "2026-11-06T11:00:00+00:00",
    "2026-11-06T11:00:00Z",
    "2026-11-06T11:00:00z",
    "2026-11-06T11:00:00.1234567+00:00",
    "2026-11-06T11:00:00.12345678+00:00",
    "2026-11-06T11:00:00.1+01:00",
    "2026-11-06T11:00:00-05:30",
    "2026-11-06T11:00:00",
    "2026-11-06T11:00",
    "2026-11-06T11:00Z",
    "2026-11-06",
    "2026-11-06T11:00:00+0100",
    "2026-11-06T11:00:00+01",
    "2026-11-06 11:00:00Z",
    "2026-11-06T24:00:00Z",
    "2026-02-30T00:00:00Z",
    "2026-11-06T11:00:60Z",
    "0001-01-01T00:00:00+00:00",
    "0001-01-01T00:00:00+01:00",
    "9999-12-31T23:59:59.9999999+00:00",
    "9999-12-31T23:59:59.9999999-01:00",
    "2026-11-06T11:00:00+14:00",
    "2026-11-06T11:00:00+14:01",
    "2026-11-06T11:00:00+15:00",
    "2026-11-06T11:00:00.Z",
    "2026-11-06T11:00:00,5Z",
    "+2026-11-06T11:00:00Z",
    "26-11-06T11:00:00Z",
    "",
    "x",
    "2026-11-06T11:00:00.0000000Z",
    "2026-11-6T11:00:00Z",
    "2026-11-06T1:00:00Z",
    " 2026-11-06T11:00:00Z",
    "2026-11-06T11:00:00Z ",
    "2026-11-06t11:00:00Z",
    "2026-11-06T11:00:00.123456789012345Z",
    "2026-11-06Z",
    "2026-11-06T11Z",
    "2026-11-06T11",
    "2026-11-06T11:00:00.5",
    "2026-11-06T11:00:00-00:00",
    "2026-11-06T11:00:00+01:30",
    "2026-11-06T11:00:00+1:00",
    "2026-11-06T23:59:59.9999999-14:00",
    "0001-01-01T00:00:00Z",
    "2026-11-06T11:00:00+00:00:00",
    "2026-13-01T00:00:00Z",
    "2026-00-01T00:00:00Z",
    "2026-11-00T00:00:00Z",
    "2024-02-29T00:00:00Z",
    "2023-02-29T00:00:00Z",
    "1900-02-29T00:00:00Z",
    "2000-02-29T00:00:00Z",
    "2026-11-06T11:00:00.1234567Z",
    "2026-11-06T11:00:00+01:00Z",
    "2026-11-06T11:00:00ZZ",
    "2026-11-06T11:00:00.1234567890123456Z",
    "2026-11-06T11:00:00.12345678901234567Z",
    "2026-11-06T11:00:00.1234567890123456+01:00",
    "2026-11-06T11:00:00.123456789012345+01:00",
    "2026-11-06T11:00:00." + new string('1', 100) + "Z",
    "2026-11-06T11:00:00." + new string('1', 2000) + "Z",
    "2026-11-06T11:00.5",
    "2026-11-06T11:00.5Z",
    "2026-11-06TZ",
    "2026-11-06T",
    "2026-11-06T11:00:00+",
    "2026-11-06T11:00:00+01:",
    "2026-11-06T11:00:00+01:0",
    "2026-11-06T11:00:00+99:99",
    "2026-11-06T11:00:00-14:01",
    "2026-11-06T11:00:00-14:00",
    "٢٠٢٦-11-06T11:00:00Z",
    "2026-11-06T11:00:00 Z",
    "2026-11-06T11:00:00Zé",
    "0000-01-01T00:00:00Z",
    "0001-01-01T00:00:00",
    "9999-12-31T23:59:59",
    "9999-12-31T23:59:59.99999999Z",
];
JsonArray parseRows = [];
foreach (string input in parseInputs)
{
    JsonObject row = new() { ["input"] = input };
    try
    {
        DateTimeOffset value = JsonSerializer.Deserialize<DateTimeOffset>(
            JsonSerializer.Serialize(input),
            options
        );
        row["utcTicks"] = value.UtcTicks.ToString(CultureInfo.InvariantCulture);
        row["offsetMinutes"] = (int)value.Offset.TotalMinutes;
        // An input without offset is read in the local time zone of the machine: record that
        // offset so the Rust test replays it instead of depending on its own time zone.
        row["localOffsetMinutes"] = (int)
            TimeZoneInfo.Local.GetUtcOffset(value.UtcDateTime).TotalMinutes;
    }
    catch (JsonException)
    {
        row["error"] = true;
    }
    parseRows.Add(row);
}

JsonObject result = new()
{
    ["generator"] = "rust/tools/format-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["doubles"] = doubleRows,
    ["round6"] = roundRows,
    ["dates"] = dateRows,
    ["parse"] = parseRows,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine(
    $"{doubleRows.Count} doubles, {roundRows.Count} roundings, {dateRows.Count} dates, {parseRows.Count} parse cases"
);
