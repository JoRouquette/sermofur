using System.Security.Cryptography;
using System.Text;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class SourceTests
{
    [Fact]
    public void AddIndexesTheFileAndIsIdempotent()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        string file = fixture.WriteFile(
            "a/notes.md",
            "Le cache est préchauffé.\n\nDeuxième paragraphe."
        );
        SourceService sources = fixture.Sources(store, a);

        MemoryRecord source = sources.Add(file, TestInstance.User);
        SourceContent content = SourceService.Content(source);
        Assert.Equal("a", source.ScopeId);
        Assert.Equal("a/notes.md", content.RelativePath);
        Assert.Equal(SourceStatus.Indexed, content.Status);
        Assert.Equal(
            Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(file))),
            content.Hash
        );
        Assert.Equal(1, content.Passages);

        MemoryRecord again = sources.Add(file, TestInstance.User);
        Assert.Equal(source, again);
        Assert.Single(sources.Show(source.Id).History);
        Assert.Equal(
            1L,
            store.Scalar($"SELECT count(*) FROM search WHERE object_id='{source.Id}'")
        );
    }

    [Fact]
    public void FileOutsideTheCurrentScopeOrInANarrowerScopeIsRefused()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        string b = fixture.Client(store, "b");
        string inA = fixture.WriteFile("a/notes.md", "text a");
        string inB = fixture.WriteFile("b/notes.md", "text b");
        string outside = Path.Combine(
            TestInstance.TempRoot,
            "sermofur-outside-" + Guid.NewGuid().ToString("N") + ".md"
        );
        File.WriteAllText(outside, "outside");
        try
        {
            // From the workspace, a file of client a would become visible to client b.
            Assert.Equal(
                "scope_boundary",
                Refusal(() => fixture.Sources(store).Add(inA, TestInstance.User))
            );
            Assert.Equal(
                "scope_boundary",
                Refusal(() => fixture.Sources(store, a).Add(inB, TestInstance.User))
            );
            Assert.Equal(
                "scope_boundary",
                Refusal(() => fixture.Sources(store, a).Add(outside, TestInstance.User))
            );
            Assert.Equal(
                "unsafe_path",
                Refusal(() =>
                    fixture
                        .Sources(store)
                        .Add(
                            Path.Combine(fixture.Root, ".sermofur", "instance.json"),
                            TestInstance.User
                        )
                )
            );
        }
        finally
        {
            File.Delete(outside);
        }
        Assert.Empty(fixture.Sources(store, a).List());
        Assert.Empty(fixture.Sources(store, b).List());
    }

    [Theory]
    [InlineData("empty", "source_rejected")]
    [InlineData("binary", "source_rejected")]
    [InlineData("latin1", "source_rejected")]
    [InlineData("large", "source_rejected")]
    [InlineData("missing", "invalid_path")]
    [InlineData("directory", "invalid_path")]
    public void UnindexableFilesAreRefusedWithAStableCode(string kind, string code)
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string path = Path.Combine(fixture.Root, "file.txt");
        switch (kind)
        {
            case "empty":
                File.WriteAllText(path, "  \n ");
                break;
            case "binary":
                File.WriteAllBytes(path, [0x41, 0x00, 0x42]);
                break;
            case "latin1":
                File.WriteAllBytes(path, Encoding.Latin1.GetBytes("café"));
                break;
            case "large":
                File.WriteAllText(path, new string('a', SourceLimits.MaxBytes + 1));
                break;
            case "directory":
                Directory.CreateDirectory(path);
                break;
        }
        Assert.Equal(code, Refusal(() => fixture.Sources(store).Add(path, TestInstance.User)));
        Assert.Empty(fixture.Sources(store).List());
    }

    [Fact]
    public void ReaderAcceptsABomAndHashesExactlyTheBytesRead()
    {
        byte[] bytes = [.. Encoding.UTF8.Preamble, .. Encoding.UTF8.GetBytes("été")];
        SourceSnapshot snapshot = FileSourceReader.Snapshot(bytes);
        Assert.Equal(SourceStatus.Indexed, snapshot.Status);
        Assert.Equal("été", Assert.Single(snapshot.Passages).Text);
        Assert.Equal(Convert.ToHexStringLower(SHA256.HashData(bytes)), snapshot.Hash);
    }

    [Fact]
    public void PassagesAreBoundedAndKeepEveryCharacter()
    {
        string paragraph = string.Concat(Enumerable.Repeat("mot 😀 ", 900));
        string text = paragraph + "\n\ncourt\n\n" + paragraph;
        IReadOnlyList<SearchDocument> passages = SourcePassages.Split(text);
        Assert.All(
            passages,
            passage => Assert.InRange(passage.Text.Length, 1, SourceLimits.MaxPassageLength)
        );
        Assert.All(passages, passage => Assert.False(char.IsHighSurrogate(passage.Text[^1])));
        Assert.Equal(
            text.Count(c => !char.IsWhiteSpace(c)),
            passages.Sum(passage => passage.Text.Count(c => !char.IsWhiteSpace(c)))
        );
        Assert.Equal(
            Enumerable.Range(0, passages.Count),
            passages.Select(passage => passage.Passage)
        );
    }

    [Fact]
    public void ReindexReportsModifiedMissingAndRestoredSourcesWithHistory()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string changing = fixture.WriteFile("changing.md", "première version");
        string leaving = fixture.WriteFile("leaving.md", "bientôt supprimé");
        SourceService sources = fixture.Sources(store);
        MemoryRecord first = sources.Add(changing, TestInstance.User);
        MemoryRecord second = sources.Add(leaving, TestInstance.User);
        string firstHash = SourceService.Content(first).Hash;

        Assert.All(
            sources.Reindex(null, "tester"),
            entry => Assert.Equal(ReindexOutcome.Unchanged, entry.Outcome)
        );

        File.WriteAllText(changing, "seconde version");
        File.Delete(leaving);
        Dictionary<Guid, ReindexEntry> entries = sources
            .Reindex(null, "tester")
            .ToDictionary(e => e.Id);
        Assert.Equal(ReindexOutcome.Modified, entries[first.Id].Outcome);
        Assert.Equal(firstHash, entries[first.Id].PreviousHash);
        Assert.NotEqual(firstHash, entries[first.Id].Hash);
        Assert.Equal(ReindexOutcome.Missing, entries[second.Id].Outcome);
        Assert.Equal(
            0L,
            store.Scalar($"SELECT count(*) FROM search WHERE object_id='{second.Id}'")
        );
        Assert.Equal(
            1L,
            store.Scalar("SELECT count(*) FROM search WHERE search MATCH '\"seconde\"'")
        );
        Assert.Equal(
            0L,
            store.Scalar("SELECT count(*) FROM search WHERE search MATCH '\"premiere\"'")
        );

        IReadOnlyList<HistoryEntry> history = sources.Show(first.Id).History;
        Assert.Equal(new[] { "create", "modified" }, history.Select(entry => entry.Reason));
        Assert.Equal(
            firstHash,
            SourceService.Content(RecordJson.Read<MemoryRecord>(history[1].PreviousState!)).Hash
        );

        File.WriteAllText(leaving, "de retour");
        Assert.Equal(
            ReindexOutcome.Restored,
            sources.Reindex(second.Id, "tester").Single().Outcome
        );
        Assert.Equal(
            "ok",
            new InstanceDoctor()
                .Inspect(fixture.Root)
                .Checks.Single(c => c.Name == "search_index")
                .Status
        );
    }

    [Fact]
    public void ReindexReadsOnlyDeclaredSourcesOfTheCurrentScope()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        fixture.Sources(store).Add(fixture.WriteFile("root.md", "racine"), TestInstance.User);
        fixture.Sources(store, a).Add(fixture.WriteFile("a/a.md", "client a"), TestInstance.User);
        fixture.WriteFile("undeclared.md", "jamais lu");
        RecordingReader reader = new RecordingReader();
        fixture.Sources(store, reader: reader).Reindex(null, "tester");
        Assert.Equal(new[] { "root.md" }, reader.Paths);
        Guid childSource = fixture.Sources(store, a).List().Single(s => s.ScopeId == "a").Id;
        Assert.Equal(
            "not_found",
            Refusal(() => fixture.Sources(store, reader: reader).Reindex(childSource, "tester"))
        );
    }

    [Fact]
    public void EvidenceFreezesTheHashOfItsSourceAndRefusesAnUnavailableOne()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string file = fixture.WriteFile("doc.md", "le service démarre en 2 s");
        MemoryRecord source = fixture.Sources(store).Add(file, TestInstance.User);
        MemoryService memory = fixture.Memory(store);
        MemoryRecord claim = memory.CreateClaim(
            TestInstance.Fact("démarrage rapide"),
            TestInstance.User,
            null
        );
        MemoryRecord evidence = memory.CreateEvidence(
            new(
                claim.Id,
                EvidenceKind.LocalDocumentation,
                "doc.md",
                "doc",
                SourceId: source.Id,
                SourceHash: "forged"
            ),
            TestInstance.User,
            null
        );
        Assert.Equal(
            SourceService.Content(source).Hash,
            RecordJson.Read<EvidenceContent>(evidence.ContentJson).SourceHash
        );
        File.Delete(file);
        fixture.Sources(store).Reindex(null, "tester");
        Assert.Equal(
            "source_unavailable",
            Refusal(() =>
                memory.CreateEvidence(
                    new(
                        claim.Id,
                        EvidenceKind.LocalDocumentation,
                        "doc.md",
                        "doc2",
                        SourceId: source.Id
                    ),
                    TestInstance.User,
                    null
                )
            )
        );
    }

    [Fact]
    public void IndexRebuildRestoresADesynchronizedIndex()
    {
        using TestInstance fixture = new TestInstance();
        using (SqliteStore store = fixture.Open())
        {
            fixture.Memory(store).CreateClaim(TestInstance.Fact("cache"), TestInstance.User, null);
            fixture
                .Sources(store)
                .Add(fixture.WriteFile("doc.md", "cache chaud"), TestInstance.User);
        }
        using (
            Microsoft.Data.Sqlite.SqliteConnection connection = new(
                $"Pooling=False;Data Source={InstanceManager.DatabasePath(fixture.Root)}"
            )
        )
        {
            connection.Open();
            using Microsoft.Data.Sqlite.SqliteCommand command = connection.CreateCommand();
            command.CommandText = "DELETE FROM search";
            command.ExecuteNonQuery();
        }
        Dictionary<string, string> before = fixture.Snapshot();
        DiagnosticCheck broken = new InstanceDoctor()
            .Inspect(fixture.Root)
            .Checks.Single(c => c.Name == "search_index");
        Assert.Equal("error", broken.Status);
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));

        CliResult rebuild = TestInstance.Run("--path", fixture.Root, "--json", "index", "rebuild");
        Assert.Equal(0, rebuild.ExitCode);
        Assert.Contains("\"indexed\": 2", rebuild.Output);
        Assert.Contains("\"changedSources\": 0", rebuild.Output);
        Assert.Equal(
            "ok",
            new InstanceDoctor()
                .Inspect(fixture.Root)
                .Checks.Single(c => c.Name == "search_index")
                .Status
        );
    }

    [Fact]
    public void SourceCommandsWorkThroughTheCli()
    {
        using TestInstance fixture = new TestInstance();
        fixture.WriteFile("doc.md", "contenu");
        CliResult add = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "source",
            "add",
            "doc.md",
            "--origin",
            "user"
        );
        Assert.Equal(0, add.ExitCode);
        Assert.Contains("\"kind\": \"source\"", add.Output);
        CliResult missingOrigin = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "source",
            "add",
            "doc.md"
        );
        Assert.Equal("invalid_arguments", TestInstance.ErrorCode(missingOrigin));
        CliResult reindex = TestInstance.Run("--path", fixture.Root, "--json", "source", "reindex");
        Assert.Contains("\"outcome\": \"unchanged\"", reindex.Output);
        CliResult list = TestInstance.Run("--path", fixture.Root, "--json", "source", "list");
        Assert.Contains("doc.md", list.Output);
    }

    [Fact]
    public void IndexRebuildReadsEverySourceAgainAndCountsOnlyVisibleObjects()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        string b = fixture.Client(store, "b");
        fixture.Sources(store, a).Add(fixture.WriteFile("a/a.md", "texte de a"), TestInstance.User);
        string inB = fixture.WriteFile("b/b.md", "texte de b");
        MemoryRecord sourceB = fixture.Sources(store, b).Add(inB, TestInstance.User);
        fixture
            .Memory(store, b)
            .CreateClaim(TestInstance.Fact("claim de b"), TestInstance.User, null);
        File.WriteAllText(inB, "texte de b modifié");

        IndexRebuild rebuild = fixture.Sources(store, a).RebuildIndex("tester");
        Assert.Equal(new IndexRebuild(1, 0), rebuild);
        Assert.Equal(
            new[] { "create", "modified" },
            fixture.Sources(store, b).Show(sourceB.Id).History.Select(entry => entry.Reason)
        );
        Assert.Contains(
            "modifié",
            Assert.Single(fixture.Recall(store, b).Recall("modifie").Results).Excerpt
        );
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("ok", report.Checks.Single(c => c.Name == "search_index").Status);
        Assert.Equal(new IndexRebuild(2, 0), fixture.Sources(store, b).RebuildIndex("tester"));
    }

    [Fact]
    public void ScopeCreatedOnTheFolderOfAnAncestorSourceTakesTheSource()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string file = fixture.WriteFile("a/notes.md", "secret de a");
        MemoryRecord source = fixture.Sources(store).Add(file, TestInstance.User);
        string a = fixture.Client(store, "a");
        string b = fixture.Client(store, "b");

        MemoryRecord moved = Assert.Single(fixture.Sources(store, a).List());
        Assert.Equal((source.Id, "a"), (moved.Id, moved.ScopeId));
        Assert.Equal("rescoped to a", fixture.Sources(store, a).Show(source.Id).History[^1].Reason);
        Assert.Empty(fixture.Recall(store, b).Recall("secret").Results);
        Assert.Empty(fixture.Recall(store).Recall("secret").Results);
        Assert.Single(fixture.Recall(store, a).Recall("secret").Results);
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("ok", report.Checks.Single(c => c.Name == "source_scopes").Status);
        Assert.Equal("ok", report.Checks.Single(c => c.Name == "search_index").Status);
    }

    [Fact]
    public void PathsCompareLikeTheFileSystem()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string upper = fixture.WriteFile("Doc.md", "majuscule");
        MemoryRecord first = fixture.Sources(store).Add(upper, TestInstance.User);
        string lower = Path.Combine(fixture.Root, "doc.md");
        if (OperatingSystem.IsWindows())
        {
            // Same file on a case-insensitive file system: the same source.
            Assert.Equal(first.Id, fixture.Sources(store).Add(lower, TestInstance.User).Id);
            return;
        }
        File.WriteAllText(lower, "minuscule");
        MemoryRecord second = fixture.Sources(store).Add(lower, TestInstance.User);
        Assert.NotEqual(first.Id, second.Id);
        Assert.NotEqual(SourceService.Content(first).Hash, SourceService.Content(second).Hash);
        Assert.All(
            fixture.Sources(store).Reindex(null, "tester"),
            entry => Assert.Equal(ReindexOutcome.Unchanged, entry.Outcome)
        );
    }

    [Fact]
    public void FileReachedThroughALinkIsRefused()
    {
        using TestInstance fixture = new TestInstance();
        string outside = Path.Combine(
            TestInstance.TempRoot,
            "sermofur-outside-" + Guid.NewGuid().ToString("N")
        );
        Directory.CreateDirectory(outside);
        File.WriteAllText(Path.Combine(outside, "secret.md"), "hors instance");
        string link = Path.Combine(fixture.Root, "linked");
        try
        {
            InstanceTests.CreateDirectoryLink(link, outside);
            using SqliteStore store = fixture.Open();
            Assert.Equal(
                "unsafe_path",
                Refusal(() =>
                    fixture.Sources(store).Add(Path.Combine(link, "secret.md"), TestInstance.User)
                )
            );
            Assert.Empty(fixture.Sources(store).List());
        }
        finally
        {
            if (new DirectoryInfo(link).LinkTarget is not null)
            {
                TestInstance.DeleteDirectoryLink(link);
            }
            Directory.Delete(outside, true);
        }
    }

    private static string Refusal(Action action) => Assert.Throws<SermofurException>(action).Code;

    private sealed class RecordingReader : ISourceReader
    {
        public List<string> Paths { get; } = [];

        public SourceSnapshot Read(string root, string relativePath)
        {
            Paths.Add(relativePath);
            return new FileSourceReader(new FileOwnership()).Read(root, relativePath);
        }
    }
}
