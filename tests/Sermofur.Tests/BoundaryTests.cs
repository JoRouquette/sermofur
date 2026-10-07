using Sermofur.Application;
using Sermofur.Cli;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class BoundaryTests
{
    [Fact]
    public void ServiceRecomputesVisibilityInsteadOfTrustingCallerList()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        fixture.Client(store, "b");
        MemoryRecord record = fixture
            .Memory(store, a)
            .CreateClaim(TestInstance.Fact("private-a"), TestInstance.User, null);
        MemoryContext forged = new MemoryContext(
            fixture.Root,
            "b",
            new HashSet<string> { "workspace", "a", "b" }
        );
        MemoryService memory = new MemoryService(store, forged);
        Assert.Empty(memory.List());
        Assert.Equal(
            "not_found",
            Assert.Throws<SermofurException>(() => memory.Get(record.Id)).Code
        );
    }

    [Fact]
    public void OversizedAndMalformedInputsCannotCreateObjects()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryService memory = fixture.Memory(store);
        Assert.Throws<SermofurException>(() =>
            memory.CreateClaim(TestInstance.Fact(new string('x', 16_385)), TestInstance.User, null)
        );
        Assert.Throws<SermofurException>(() =>
            memory.CreateClaim(
                TestInstance.Fact() with
                {
                    Category = (MemoryCategory)999,
                },
                TestInstance.User,
                null
            )
        );
        Assert.Throws<SermofurException>(() => CommandArguments.ParseEnum<ActorKind>("1"));
        Assert.Empty(memory.List());
    }

    [Fact]
    public void ParserFuzzFailsWithoutUnhandledExceptionsOrWrites()
    {
        using TestInstance fixture = new TestInstance();
        Dictionary<string, string> before = fixture.Snapshot();
        Random random = new Random(17);
        for (int i = 0; i < 100; i++)
        {
            using StringWriter stdout = new StringWriter();
            using StringWriter stderr = new StringWriter();
            string text = new string(
                Enumerable
                    .Range(0, random.Next(1, 40))
                    .Select(_ => (char)random.Next(33, 127))
                    .ToArray()
            );
            int exit = new CommandRunner(stdout, stderr).Run([
                "--path",
                fixture.Root,
                "claim",
                "show",
                text,
                "--json",
            ]);
            Assert.Equal(1, exit);
            Assert.Equal("", stdout.ToString());
        }
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public async Task ConcurrentHostProcessesPersistAllClaims()
    {
        using TestInstance fixture = new TestInstance();
        CliResult[] results = await Task.WhenAll(
            Enumerable
                .Range(0, 8)
                .Select(i =>
                    TestInstance.RunCli(
                        "--path",
                        fixture.Root,
                        "claim",
                        "add",
                        "process " + i,
                        "--origin",
                        "user",
                        "--key",
                        "process-" + i,
                        "--json"
                    )
                )
        );
        Assert.All(results, result => Assert.Equal(0, result.ExitCode));
        using SqliteStore store = fixture.Open(true);
        Assert.Equal(8, fixture.Memory(store).List().Count);
    }

    [Fact]
    public void ForeignKeyEnforcementRejectsOrphanEvidence()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryRecord candidate = new MemoryRecord(
            Guid.NewGuid(),
            "workspace",
            RecordKind.Evidence,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(
                new EvidenceContent(Guid.NewGuid(), EvidenceKind.SourceCode, "ref", "root")
            ),
            TestInstance.User
        );
        Assert.Throws<SermofurException>(() => store.CreateRecord(candidate, null));
        Assert.Empty(fixture.Memory(store).List());
    }

    [Fact]
    public void StaleKnowledgeLosesApplicabilityWithoutDeletion()
    {
        MemoryRecord record = new MemoryRecord(
            Guid.NewGuid(),
            "workspace",
            RecordKind.Claim,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(
                TestInstance.Fact() with
                {
                    ReviewAfter = DateTimeOffset.UtcNow.AddMinutes(-1),
                }
            ),
            TestInstance.User
        );
        Assert.Equal(ConfidenceLevel.Low, MemoryService.EvaluateConfidence(record, []).Level);
        Assert.Contains("revalidation", MemoryService.EvaluateConfidence(record, []).Reasons[0]);
    }

    [Fact]
    public void ProjectAndSingleRepositoryMayShareExistingDirectory()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string client = fixture.Client(store, "acme");
        string project = Path.Combine(client, "billing-api");
        Directory.CreateDirectory(project);
        fixture
            .Scopes(store, client)
            .Register(new Scope("billing", ScopeKind.Project, "acme", "acme/billing-api"));
        fixture
            .Scopes(store, project)
            .Register(
                new Scope("billing-repo", ScopeKind.Repository, "billing", "acme/billing-api")
            );
        MemoryContext context = fixture.Manager.ResolveContext(
            project,
            fixture.Root,
            store.ReadScopes()
        );
        Assert.Equal("billing-repo", context.ScopeId);
        Assert.Equal(
            new[] { "acme", "billing", "billing-repo", "workspace" },
            context.VisibleScopes.Order().ToArray()
        );
        Assert.Equal("healthy_with_warnings", new InstanceDoctor().Inspect(fixture.Root).Overall);
    }

    [Fact]
    public void ProjectionWriteFailureLeavesCommittedObjectRecoverable()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string recordsDirectory = Path.Combine(fixture.Root, ".sermofur", "records");
        Directory.Delete(recordsDirectory);
        File.WriteAllText(recordsDirectory, "projection blocker");
        MemoryRecord record = new MemoryRecord(
            Guid.NewGuid(),
            "workspace",
            RecordKind.Claim,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(TestInstance.Fact()),
            TestInstance.User
        );
        Assert.Equal(
            "projection_pending",
            Assert.Throws<SermofurException>(() => store.CreateRecord(record, "recovery")).Code
        );
        Assert.Equal(record, fixture.Memory(store).Get(record.Id));
        Assert.Single(fixture.Memory(store).Explain(record.Id).History);
        File.Delete(recordsDirectory);
        fixture.Memory(store).Export();
        Assert.True(new MarkdownProjection(fixture.Root).IsCurrent(record));
    }

    [Fact]
    public async Task ConcurrentSameKeyIngestionProducesOneRecordAndOneHistoryEntry()
    {
        using TestInstance fixture = new TestInstance();
        MemoryRecord[] records = await Task.WhenAll(
            Enumerable
                .Range(0, 8)
                .Select(_ =>
                    Task.Run(() =>
                    {
                        using SqliteStore store = fixture.Open();
                        return fixture
                            .Memory(store)
                            .CreateClaim(TestInstance.Fact("same"), TestInstance.User, "same-key");
                    })
                )
        );
        Assert.Single(records.Select(r => r.Id).Distinct());
        using SqliteStore read = fixture.Open(true);
        Assert.Single(fixture.Memory(read).List());
        Assert.Single(fixture.Memory(read).Explain(records[0].Id).History);
    }
}
