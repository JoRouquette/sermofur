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

        IndexRebuild rebuild = fixture.Sources(store, a).RebuildIndex();
        Assert.Equal(new IndexRebuild(1, 0), rebuild);
        IReadOnlyList<HistoryEntry> history = fixture.Sources(store, b).Show(sourceB.Id).History;
        Assert.Equal(new[] { "create", "modified" }, history.Select(entry => entry.Reason));
        // An instance operation: recorded under the system actor, not a user of one scope.
        Assert.Equal(SourceService.SystemActor, history[^1].Actor);
        Assert.Contains(
            "modifié",
            Assert.Single(fixture.Recall(store, b).Recall("modifie").Results).Excerpt
        );
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("ok", report.Checks.Single(c => c.Name == "search_index").Status);
        Assert.Equal(new IndexRebuild(2, 0), fixture.Sources(store, b).RebuildIndex());
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

    [Theory]
    [InlineData("Doc.md", "doc.md")]
    [InlineData("Dossier/Été.md", "dossier/été.md")]
    // Decomposed on disk (NFD, as some macOS tools write it), composed when typed (NFC).
    [InlineData("Dossier/E\u0301te\u0301.md", "dossier/été.md")]
    public void OneFileIsOneSourceWhateverTheCaseTyped(string onDisk, string typed)
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryRecord first = fixture
            .Sources(store)
            .Add(fixture.WriteFile(onDisk, "contenu"), TestInstance.User);
        string other = Path.Combine(fixture.Root, typed);
        // The file system decides, not the operating system: macOS is case-insensitive by
        // default, Linux is not.
        bool caseInsensitive = File.Exists(other);
        if (caseInsensitive)
        {
            MemoryRecord again = fixture.Sources(store).Add(other, TestInstance.User);
            Assert.Equal(first.Id, again.Id);
            Assert.Equal(onDisk, SourceService.Content(again).RelativePath);
            Assert.Single(fixture.Sources(store).List());
            return;
        }
        Directory.CreateDirectory(Path.GetDirectoryName(other)!);
        File.WriteAllText(other, "autre contenu");
        MemoryRecord second = fixture.Sources(store).Add(other, TestInstance.User);
        Assert.NotEqual(first.Id, second.Id);
        // The folder may still be matched without case (Windows); the file name is the typed one.
        Assert.EndsWith(
            "/" + Path.GetFileName(typed),
            "/" + SourceService.Content(second).RelativePath
        );
        Assert.All(
            fixture.Sources(store).Reindex(null, "tester"),
            entry => Assert.Equal(ReindexOutcome.Unchanged, entry.Outcome)
        );
    }

    [Fact]
    public void ScopeMappingTypedInAnotherCaseStillHoldsItsFolder()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string file = fixture.WriteFile("client/notes.md", "secret du client");
        if (!Directory.Exists(Path.Combine(fixture.Root, "CLIENT")))
        {
            // Case-sensitive file system: CLIENT is another folder, nothing to compare.
            return;
        }
        Scope registered = fixture
            .Scopes(store)
            .Register(new Scope("c", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, "CLIENT"));
        Assert.Equal("client", registered.RelativePath);
        Assert.Equal(
            "scope_boundary",
            Refusal(() => fixture.Sources(store).Add(file, TestInstance.User))
        );
        Assert.Equal(
            "ok",
            new InstanceDoctor()
                .Inspect(fixture.Root)
                .Checks.Single(c => c.Name == "scope_mappings")
                .Status
        );
        Tamper(fixture, "UPDATE scopes SET relative_path='CLIENT' WHERE id='c'");
        // On Windows the comparison absorbs case: the stored variant still holds its folder.
        Assert.Equal(
            OperatingSystem.IsWindows() ? "ok" : "warning",
            new InstanceDoctor()
                .Inspect(fixture.Root)
                .Checks.Single(c => c.Name == "scope_mappings")
                .Status
        );
    }

    [Fact]
    public void MappingsCompareAsNamedOnDiskForChildrenAndDuplicates()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "client", "sub"));
        if (!Directory.Exists(Path.Combine(fixture.Root, "CLIENT")))
        {
            // Case-sensitive file system: CLIENT is another folder, nothing to compare.
            return;
        }
        // Only macOS (case-insensitive, ordinal comparison) discriminates: on Windows the
        // comparison already ignores case.
        fixture
            .Scopes(store)
            .Register(new Scope("c", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, "CLIENT"));
        // The parent as an earlier version stored it: under the typed spelling.
        Tamper(fixture, "UPDATE scopes SET relative_path='CLIENT' WHERE id='c'");
        Scope child = fixture
            // Context path in the stored spelling, so that the context resolves to c.
            .Scopes(store, Path.Combine(fixture.Root, "CLIENT"))
            .Register(new Scope("p", ScopeKind.Project, "c", "client/sub"));
        Assert.Equal("client/sub", child.RelativePath);
        // A mapping stored under another spelling by an earlier version still collides.
        SermofurException duplicate = Assert.Throws<SermofurException>(() =>
            fixture
                .Scopes(store)
                .Register(new Scope("d", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, "client"))
        );
        Assert.Equal(ScopeService.DuplicateMapping, duplicate.Code);
    }

    [Theory]
    [InlineData(".SERMOFUR/instance.json")]
    [InlineData(".Sermofur/memory.db")]
    public void SermofurFilesCannotBeReachedThroughACaseVariant(string typed)
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        Assert.Equal(
            "unsafe_path",
            Refusal(() =>
                fixture.Sources(store).Add(Path.Combine(fixture.Root, typed), TestInstance.User)
            )
        );
    }

    [Fact]
    public void IndexRebuildDropsALostSourceAndRollsBackOnFailure()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string kept = fixture.WriteFile("kept.md", "texte conservé");
        string lost = fixture.WriteFile("lost.md", "texte perdu");
        MemoryRecord keptSource = fixture.Sources(store).Add(kept, TestInstance.User);
        MemoryRecord lostSource = fixture.Sources(store).Add(lost, TestInstance.User);
        File.WriteAllText(kept, "texte conservé et révisé");

        // Reading lost.md fails: whatever the order, the change of kept.md is rolled back too.
        Assert.Throws<IOException>(() =>
            fixture.Sources(store, reader: new FailingReader("lost.md")).RebuildIndex()
        );
        Assert.Single(fixture.Sources(store).Show(keptSource.Id).History);
        Assert.Empty(fixture.Recall(store).Recall("révisé").Results);
        Assert.Single(fixture.Recall(store).Recall("conservé").Results);
        Assert.Single(fixture.Recall(store).Recall("perdu").Results);

        File.Delete(lost);
        Assert.Equal(new IndexRebuild(1, 2), fixture.Sources(store).RebuildIndex());
        Assert.Empty(fixture.Recall(store).Recall("perdu").Results);
        Assert.Single(fixture.Recall(store).Recall("révisé").Results);
        HistoryEntry entry = fixture.Sources(store).Show(lostSource.Id).History[^1];
        Assert.Equal(("missing", SourceService.SystemActor), (entry.Reason, entry.Actor));
        Assert.Equal(
            "ok",
            new InstanceDoctor()
                .Inspect(fixture.Root)
                .Checks.Single(c => c.Name == "search_index")
                .Status
        );
    }

    private sealed class FailingReader(string failingPath) : ISourceReader
    {
        private readonly FileSourceReader inner = new FileSourceReader(new FileOwnership());

        public SourceSnapshot Read(string root, string relativePath) =>
            relativePath == failingPath
                ? throw new IOException("simulated read failure")
                : inner.Read(root, relativePath);
    }

    [Fact]
    public void ReindexSkipsASourceChangedByAnotherCommandMeanwhile()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string file = fixture.WriteFile("doc.md", "contenu");
        MemoryRecord source = fixture.Sources(store).Add(file, TestInstance.User);
        File.WriteAllText(file, "contenu modifié");
        RacingStore racing = new(
            store,
            InstanceManager.DatabasePath(fixture.Root),
            "UPDATE records SET revision=revision+1, payload=json_set(payload,'$.revision',revision+1) WHERE kind='Source'",
            raceSaveSource: true
        );
        SourceService service = new(
            racing,
            new FileSourceReader(new FileOwnership()),
            new LocalPathResolver(),
            fixture.Context(store)
        );
        ReindexEntry entry = Assert.Single(service.Reindex(null, "tester"));
        Assert.Equal(ReindexOutcome.Skipped, entry.Outcome);
        Assert.Equal(SourceService.Content(source).Hash, entry.Hash);
        Assert.Single(fixture.Sources(store).Show(source.Id).History);
    }

    [Fact]
    public void ScopeCreatedDuringASourceAddIsSeenUnderTheWriteLock()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string file = fixture.WriteFile("a/notes.md", "secret de a");
        // The service read the scopes before a concurrent scope add; the rival insertion
        // happens just before the SaveSource transaction.
        RacingStore racing = new(
            store,
            InstanceManager.DatabasePath(fixture.Root),
            "INSERT INTO scopes VALUES('rival','Client','workspace','a')",
            raceSaveSource: true
        );
        SourceService service = new(
            racing,
            new FileSourceReader(new FileOwnership()),
            new LocalPathResolver(),
            fixture.Context(store)
        );
        Assert.Equal("scope_boundary", Refusal(() => service.Add(file, TestInstance.User)));
        Assert.True(racing.PreconditionSawRival);
        Assert.Empty(
            store.ReadRecords(new HashSet<string> { "workspace", "rival" }, RecordKind.Source)
        );
    }

    [Fact]
    public void DoctorFindsASourceLeftInABroaderScopeAndAMisplacedIndexEntry()
    {
        using TestInstance fixture = new TestInstance();
        using (SqliteStore store = fixture.Open())
        {
            fixture.Client(store, "a");
            fixture
                .Sources(store, Path.Combine(fixture.Root, "a"))
                .Add(fixture.WriteFile("a/notes.md", "secret de a"), TestInstance.User);
        }
        Tamper(fixture, "UPDATE search SET scope_id='workspace'");
        DoctorReport index = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("error", index.Checks.Single(c => c.Name == "search_index").Status);
        Assert.Equal("ok", index.Checks.Single(c => c.Name == "source_scopes").Status);

        Tamper(
            fixture,
            "UPDATE records SET scope_id='workspace', payload=json_set(payload,'$.scopeId','workspace') WHERE kind='Source'"
        );
        DoctorReport scopes = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("error", scopes.Checks.Single(c => c.Name == "source_scopes").Status);
    }

    private static void Tamper(TestInstance fixture, string sql)
    {
        using Microsoft.Data.Sqlite.SqliteConnection connection = new(
            $"Pooling=False;Data Source={InstanceManager.DatabasePath(fixture.Root)}"
        );
        connection.Open();
        using Microsoft.Data.Sqlite.SqliteCommand command = connection.CreateCommand();
        command.CommandText = sql;
        Assert.True(command.ExecuteNonQuery() > 0);
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
