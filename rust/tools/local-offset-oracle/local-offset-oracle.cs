// Records the offset .NET gives the local time zone (decision A5, lot 7.2): for instants, as
// DateTimeOffset.Now and ToLocalTime give it (TimeZoneInfo.Local.GetUtcOffset), and for clock
// readings written without offset, as System.Text.Json reads them (DateTimeOffset.Parse with the
// invariant culture is checked to agree). Probes: fixed historic and future dates, then every
// transition of the zone in 1985, 1986, 1996, 2010, 2011, 2014 and 2024 to 2027, at -3600,
// -60, -1, 0, +1, +60 and +3600 s, and the clock readings across each skipped or repeated
// hour, every 15 minutes and at its edges.
// Windows records the zone of the machine. Linux records the zone of the machine, then runs
// itself again under TZ=Europe/Paris, America/New_York, Australia/Lord_Howe, Asia/Kathmandu,
// Europe/Dublin (negative DST in the tz database), Africa/Casablanca and Europe/Moscow (2011
// and 2014: changes of standard time).
// Ticks are the 100-ns ticks of DateTime (UTC ticks for instants, local ticks for clocks).
// Usage: dotnet run local-offset-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
using System.Diagnostics;
using System.Globalization;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;

if (args.Length == 1 && args[0] == "--zone")
{
    Console.Out.Write(Zone().ToJsonString());
    return;
}

JsonArray zones = [Zone()];
if (!OperatingSystem.IsWindows())
{
    foreach (
        string tz in new[]
        {
            "Europe/Paris",
            "America/New_York",
            "Australia/Lord_Howe",
            "Asia/Kathmandu",
            "Europe/Dublin",
            "Africa/Casablanca",
            "Europe/Moscow",
        }
    )
    {
        ProcessStartInfo start = new ProcessStartInfo(Environment.ProcessPath!, ["--zone"])
        {
            RedirectStandardOutput = true,
            UseShellExecute = false,
        };
        start.Environment["TZ"] = tz;
        using Process child = Process.Start(start)!;
        string text = child.StandardOutput.ReadToEnd();
        child.WaitForExit();
        if (child.ExitCode != 0)
        {
            throw new InvalidOperationException($"TZ={tz}: exit {child.ExitCode}");
        }
        JsonObject zone = JsonNode.Parse(text)!.AsObject();
        zone["tz"] = tz;
        zones.Add(zone);
    }
}

File.WriteAllText(
    args[0],
    new JsonObject
    {
        ["os"] = OperatingSystem.IsWindows() ? "windows" : "unix",
        ["zones"] = zones,
    }.ToJsonString(
        new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        }
    ).Replace("\r\n", "\n") + "\n"
);

static JsonObject Zone()
{
    TimeZoneInfo zone = TimeZoneInfo.Local;
    SortedSet<long> instants = [];
    SortedSet<long> clocks = [];
    void Both(DateTime value)
    {
        instants.Add(value.Ticks);
        clocks.Add(value.Ticks);
    }
    foreach (
        string text in new[]
        {
            "0001-01-02T12:00:00",
            "1900-01-01T00:00:00",
            "1912-01-01T00:00:00",
            "1940-06-15T00:00:00",
            "1970-01-01T00:00:00",
            "1976-07-01T00:00:00",
            "2000-01-01T00:00:00",
            "2026-11-10T12:00:00",
            "2100-01-01T00:00:00",
            "2100-07-01T00:00:00",
            "9999-12-30T00:00:00",
        }
    )
    {
        Both(DateTime.Parse(text, CultureInfo.InvariantCulture));
    }
    foreach (int year in new[] { 1985, 1986, 1996, 2010, 2011, 2014, 2024, 2025, 2026, 2027 })
    {
        DateTime t = new DateTime(year, 1, 1, 0, 0, 0, DateTimeKind.Utc);
        TimeSpan last = zone.GetUtcOffset(t);
        while (t.Year == year)
        {
            DateTime next = t.AddMinutes(15);
            TimeSpan offset = zone.GetUtcOffset(next);
            if (offset != last)
            {
                DateTime lo = t,
                    hi = next;
                while ((hi - lo).TotalSeconds > 1)
                {
                    DateTime mid = lo.AddTicks((hi - lo).Ticks / 2);
                    mid = mid.AddTicks(-(mid.Ticks % TimeSpan.TicksPerSecond));
                    if (zone.GetUtcOffset(mid) == last)
                    {
                        lo = mid;
                    }
                    else
                    {
                        hi = mid;
                    }
                }
                foreach (int s in new[] { -3600, -60, -1, 0, 1, 60, 3600 })
                {
                    instants.Add(hi.AddSeconds(s).Ticks);
                }
                DateTime before = hi + last;
                DateTime after = hi + offset;
                DateTime from = before < after ? before : after;
                DateTime to = before < after ? after : before;
                foreach (int s in new[] { -3600, -60, -1 })
                {
                    clocks.Add(from.AddSeconds(s).Ticks);
                }
                for (DateTime w = from; w <= to; w = w.AddMinutes(15))
                {
                    clocks.Add(w.Ticks);
                }
                foreach (int s in new[] { -1, 0, 1, 60, 3600 })
                {
                    clocks.Add(to.AddSeconds(s).Ticks);
                }
                last = offset;
            }
            t = next;
        }
    }
    JsonArray instantProbes = [];
    foreach (long ticks in instants)
    {
        DateTime utc = new DateTime(ticks, DateTimeKind.Utc);
        DateTimeOffset local = new DateTimeOffset(utc).ToLocalTime();
        if (local.Offset != zone.GetUtcOffset(utc))
        {
            throw new InvalidOperationException($"ToLocalTime and GetUtcOffset differ at {ticks}");
        }
        instantProbes.Add(
            new JsonObject
            {
                ["utc_ticks"] = ticks,
                ["offset_minutes"] = (int)local.Offset.TotalMinutes,
                ["text"] = local.ToString("yyyy-MM-dd'T'HH:mm:ss.fffzzz", CultureInfo.InvariantCulture),
            }
        );
    }
    JsonArray clockProbes = [];
    foreach (long ticks in clocks)
    {
        string text = new DateTime(ticks).ToString(
            "yyyy-MM-dd'T'HH:mm:ss",
            CultureInfo.InvariantCulture
        );
        DateTimeOffset read = JsonSerializer.Deserialize<DateTimeOffset>($"\"{text}\"");
        DateTimeOffset parsed = DateTimeOffset.Parse(text, CultureInfo.InvariantCulture);
        if (read != parsed || read.Offset != parsed.Offset)
        {
            throw new InvalidOperationException($"System.Text.Json and Parse differ on {text}");
        }
        clockProbes.Add(
            new JsonObject
            {
                ["local_ticks"] = ticks,
                ["text"] = text,
                ["offset_minutes"] = (int)read.Offset.TotalMinutes,
                ["kind"] =
                    zone.IsAmbiguousTime(new DateTime(ticks)) ? "ambiguous"
                    : zone.IsInvalidTime(new DateTime(ticks)) ? "skipped"
                    : "single",
            }
        );
    }
    return new JsonObject
    {
        ["zone"] = zone.Id,
        ["tz"] = null,
        ["instants"] = instantProbes,
        ["clocks"] = clockProbes,
    };
}
