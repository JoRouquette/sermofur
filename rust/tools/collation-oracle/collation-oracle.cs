// Derives, from the answers of CompareInfo (InvariantCulture, IgnoreCase | IgnoreNonSpace, the
// comparison of RecallService.Excerpt), what each Unicode scalar value weighs: nothing (an
// ignorable), one weight, or several (an expansion such as ß = ss or ﬁ = fi). Nothing is written
// by hand:
// 1. The sort key of every scalar is read (GetSortKey). At these options it holds the primary
//    weights only, with the lead-byte compression of ICU. Whether a lead byte compresses is read
//    from the key of a doubled character.
// 2. Scalars are taken by increasing key length. A key that splits into the keys of shorter
//    atomic scalars (compression applied between them) is an expansion if the oracle agrees: the
//    key of the string of those scalars is the key of the scalar. Otherwise the key is atomic.
// 3. Every split is checked again on strings: each scalar followed and preceded by each of a set
//    of partners. A pair whose key is not the concatenation is context-dependent (a contraction
//    or a reordering): it is recorded, never modelled.
// 4. A weight is named by the smallest scalar whose key it is. The table lists every scalar whose
//    weights are not its own name.
// 5. IndexOf answers on strings built from table entries are recorded as probes, so that the
//    Rust matcher is checked against the call the excerpt makes.
// Usage: dotnet run collation-oracle.cs -- TABLE.txt PROBES.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

CompareInfo compare = CultureInfo.InvariantCulture.CompareInfo;
const CompareOptions Options = CompareOptions.IgnoreCase | CompareOptions.IgnoreNonSpace;
const byte Low = 0x03;
const byte High = 0xFF;

byte[] Key(string text)
{
    byte[] data = compare.GetSortKey(text, Options).KeyData;
    if (data.Length == 0 || data[^1] != 0 || data[..^1].Any(b => b < 2))
    {
        throw new InvalidOperationException($"Unexpected key layout for {Hex(text)}.");
    }
    return data[..^1];
}

string S(int scalar) => char.ConvertFromUtf32(scalar);
string Hex(string text) =>
    string.Join(" ", text.EnumerateRunes().Select(r => r.Value.ToString("X4")));

int[] scalars = Enumerable.Range(0, 0x110000).Where(c => c is < 0xD800 or > 0xDFFF).ToArray();
Dictionary<int, byte[]> keys = scalars.ToDictionary(c => c, c => Key(S(c)));

// A lead byte compresses when a doubled character omits it the second time.
bool[] compressible = new bool[256];
foreach (
    IGrouping<byte, int> lead in scalars.Where(c => keys[c].Length > 1).GroupBy(c => keys[c][0])
)
{
    int c = lead.MinBy(c => keys[c].Length);
    byte[] single = keys[c];
    byte[] doubled = Key(S(c) + S(c));
    compressible[lead.Key] = doubled.SequenceEqual(single.Concat(single.Skip(1)));
    if (!compressible[lead.Key] && !doubled.SequenceEqual(single.Concat(single)))
    {
        throw new InvalidOperationException($"Lead byte {lead.Key:X2}: unexpected doubled key.");
    }
}

// The key of a list of atomic keys, compressed as ICU writes it.
byte[] Encode(IEnumerable<string> pieces)
{
    List<byte> key = new List<byte>();
    int run = 0;
    foreach (byte[] piece in pieces.Select(Convert.FromHexString))
    {
        if (run != 0 && piece[0] == run)
        {
            key.AddRange(piece.Skip(1));
            continue;
        }
        if (run != 0)
        {
            key.Add(piece[0] < run ? Low : High);
        }
        key.AddRange(piece);
        run = compressible[piece[0]] ? piece[0] : 0;
    }
    return [.. key];
}

// Splits of a key into at least two atomic keys, by the compression rule; first found first.
Dictionary<string, int> atomic = new Dictionary<string, int>();
IEnumerable<List<string>> Splits(byte[] key, int position, int run)
{
    if (position == key.Length)
    {
        yield return [];
        yield break;
    }
    if (run != 0 && (key[position] == Low || key[position] == High))
    {
        foreach (List<string> rest in Splits(key, position + 1, 0))
        {
            yield return rest;
        }
        yield break;
    }
    for (int end = key.Length; end > position; end--)
    {
        byte[] piece = run != 0 ? [(byte)run, .. key[position..end]] : key[position..end];
        string hex = Convert.ToHexString(piece);
        if (!atomic.ContainsKey(hex))
        {
            continue;
        }
        int next =
            run != 0 ? run
            : compressible[piece[0]] ? piece[0]
            : 0;
        foreach (List<string> rest in Splits(key, end, next))
        {
            yield return [hex, .. rest];
        }
    }
}

Dictionary<int, List<string>> weightsOf = new Dictionary<int, List<string>>();
foreach (int c in scalars.OrderBy(c => keys[c].Length).ThenBy(c => c))
{
    byte[] key = keys[c];
    string hex = Convert.ToHexString(key);
    if (key.Length == 0)
    {
        weightsOf[c] = [];
        continue;
    }
    if (atomic.ContainsKey(hex))
    {
        weightsOf[c] = [hex];
        continue;
    }
    List<string>? split = Splits(key, 0, 0)
        .Where(pieces => pieces.Count > 1)
        .Take(8)
        .FirstOrDefault(pieces =>
            Key(string.Concat(pieces.Select(p => S(atomic[p])))).SequenceEqual(key)
        );
    if (split is null)
    {
        atomic[hex] = c;
        weightsOf[c] = [hex];
    }
    else
    {
        weightsOf[c] = split;
    }
}
foreach (int c in scalars)
{
    if (!Encode(weightsOf[c]).SequenceEqual(keys[c]))
    {
        throw new InvalidOperationException($"Key of U+{c:X4} does not encode back.");
    }
}

// Pairs with partners from several scripts: context-dependent ones are recorded.
int[] partners =
[
    'a',
    'Z',
    '1',
    '-',
    0xDF,
    0xE9,
    0x0301,
    0x00AD,
    0x0438,
    0x03A9,
    0x0627,
    0x0E01,
    0x0915,
    0x4E00,
    0xAC00,
    0x1100,
    0xBD,
    0xFB03,
    0x1D400,
];
JsonArray contextual = [];
HashSet<int> contextualScalars = new HashSet<int>();
foreach (int c in scalars)
{
    foreach (int p in partners)
    {
        foreach ((int first, int second) in new[] { (c, p), (p, c) })
        {
            byte[] expected = Encode(weightsOf[first].Concat(weightsOf[second]));
            if (!Key(S(first) + S(second)).SequenceEqual(expected))
            {
                contextual.Add($"{first:X4} {second:X4}");
                contextualScalars.Add(c);
            }
        }
    }
}

// Names of the weights: the smallest scalar whose key it is.
Dictionary<string, int> name = atomic;

StringBuilder table = new StringBuilder();
table.Append(
    "# Generated by rust/tools/collation-oracle from CompareInfo of the invariant culture\n"
);
table
    .Append("# (IgnoreCase | IgnoreNonSpace), ")
    .Append(System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription)
    .Append(". Do not edit.\n");
table.Append("# Line: scalar, then the names of its primary weights (none: ignorable). A scalar\n");
table.Append("# absent from the table weighs one weight named after itself. Hexadecimal.\n");
int entries = 0;
int ignorables = 0;
int expansions = 0;
List<int> listed = new List<int>();
foreach (int c in scalars)
{
    List<string> weights = weightsOf[c];
    if (weights.Count == 1 && name[weights[0]] == c)
    {
        continue;
    }
    entries++;
    ignorables += weights.Count == 0 ? 1 : 0;
    expansions += weights.Count > 1 ? 1 : 0;
    listed.Add(c);
    table.Append(c.ToString("X"));
    foreach (string weight in weights)
    {
        table.Append(' ').Append(name[weight].ToString("X"));
    }
    table.Append('\n');
}
File.WriteAllText(args[0], table.ToString(), new UTF8Encoding(false));

// IndexOf probes on a sample of the table, context-dependent scalars left out.
string Plain(IEnumerable<string> weights) => string.Concat(weights.Select(w => S(name[w])));
JsonArray probes = [];
void Probe(string text, string term) =>
    probes.Add(new JsonArray(text, term, compare.IndexOf(text, term, Options)));
int[] sample = listed
    .Where((c, i) => i % 8 == 0 || (weightsOf[c].Count > 1 && i % 4 == 0))
    .Where(c => !contextualScalars.Contains(c))
    .ToArray();
foreach (int c in sample)
{
    string s = S(c);
    List<string> weights = weightsOf[c];
    if (weights.Count == 0)
    {
        Probe("a" + s + "b", "ab");
        Probe(s + "ab", "ab");
        Probe("ab" + s, "b");
        continue;
    }
    string plain = Plain(weights);
    Probe("x" + s + "y", plain);
    Probe(plain + " " + s, s);
    Probe("\u00AD" + s, plain);
    if (weights.Count > 1)
    {
        Probe("z" + s, Plain(weights.Take(1)));
        Probe(s + s, Plain(weights.TakeLast(1)) + Plain(weights.Take(1)));
        Probe("s" + s, Plain(weights.TakeLast(1)));
    }
}
foreach (
    (string text, string term) in new (string, string)[]
    {
        ("ß", "s"),
        ("ß", "ss"),
        ("aßb", "ss"),
        ("ss", "ß"),
        ("sß", "ss"),
        ("ßs", "ss"),
        ("sss", "ß"),
        ("\u00ADabc", "abc"),
        ("x\u00ADabc", "abc"),
        ("e\u0301x", "e"),
        ("é", "e"),
        ("e", "é"),
        ("ǆ", "d"),
        ("Ǆungla", "dž"),
        ("xæ", "ae"),
        ("ﬁn", "fi"),
        ("ﬁn", "f"),
        ("ﬁn", "in"),
        ("ﬃ", "ff"),
        ("ﬃ", "ffi"),
        ("a\u0000b", "ab"),
        ("ab", "a\u0000b"),
        ("\u200Bab", "ab"),
        ("a-b", "ab"),
        ("a b", "ab"),
        ("가", "ᄀ"),
        ("가", "가"),
        ("x㎏", "kg"),
        ("ł ø đ ё", "l o d е"),
        ("abc", "\u00AD"),
    }
)
{
    Probe(text, term);
}

// Sequences whose boundaries follow grapheme clusters: Hangul jamo, regional indicators, emoji
// joined by ZWJ or with a skin tone, a spacing mark after its base, CR LF, a prepended mark.
foreach (
    (string text, string[] terms) in new (string, string[])[]
    {
        ("x각y", ["ᄀ", "ᅡ", "ᆨ", "가", "ᅡᆨ", "가", "각"]),
        ("각", ["ᆨ", "가", "각"]),
        ("ᄀ가", ["가", "ᄀ", "가"]),
        (
            "\U0001F1EB\U0001F1F7\U0001F1E9\U0001F1EA",
            ["\U0001F1F7\U0001F1E9", "\U0001F1E9\U0001F1EA", "\U0001F1EB", "\U0001F1F7"]
        ),
        (
            "x\U0001F469\u200D\U0001F4BBy",
            ["\U0001F4BB", "\U0001F469", "\U0001F469\u200D\U0001F4BB", "\u200D\U0001F4BB"]
        ),
        ("\U0001F468\u200D\U0001F469\u200D\U0001F467", ["\U0001F469", "\U0001F467"]),
        ("x\U0001F44D\U0001F3FDy", ["\U0001F44D", "\U0001F3FD", "\U0001F44D\U0001F3FD"]),
        ("xक\u093Fy", ["क", "\u093F", "क\u093F"]),
        ("x\u0903y", ["\u0903"]),
        ("a\r\nb", ["\nb", "a\r", "\r\n", "b", "\n"]),
        ("x\u0601ab", ["ab", "b"]),
        ("x\u1DD5y", ["AO", "y"]),
    }
)
{
    foreach (string term in terms)
    {
        Probe(text, term);
    }
}

JsonObject result = new()
{
    ["generator"] = "rust/tools/collation-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["entries"] = entries,
    ["ignorables"] = ignorables,
    ["expansions"] = expansions,
    ["partners"] = new JsonArray([.. partners.Select(p => (JsonNode)p.ToString("X4"))]),
    ["contextual"] = contextual,
    ["probes"] = probes,
};
File.WriteAllText(
    args[1],
    result.ToJsonString(
        new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        }
    ) + "\n",
    new UTF8Encoding(false)
);
Console.WriteLine(
    $"{entries} entries ({ignorables} ignorables, {expansions} expansions), {contextual.Count} contextual pairs, {probes.Count} probes"
);
