using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class MemoryTests
{
    [Fact]
    public void PersistsClaimsEvidenceRetexAndHistoryAcrossConnections()
    {
        using TestInstance fixture = new TestInstance();
        Guid id;
        using (SqliteStore store = fixture.Open())
        {
            MemoryService memory = fixture.Memory(store);
            MemoryRecord claim = memory.CreateClaim(TestInstance.Fact(), TestInstance.User, "once");
            id = claim.Id;
            MemoryRecord repeated = memory.CreateClaim(
                TestInstance.Fact(),
                TestInstance.User,
                "once"
            );
            Assert.Equal(id, repeated.Id);
            Assert.Equal(
                "idempotency_conflict",
                Assert
                    .Throws<SermofurException>(() =>
                        memory.CreateClaim(
                            TestInstance.Fact("different"),
                            TestInstance.User,
                            "once"
                        )
                    )
                    .Code
            );
            memory.CreateEvidence(
                new(id, EvidenceKind.SourceCode, "reference://code", "code-root"),
                TestInstance.User,
                null
            );
            memory.CreateRetex(
                new("test failed", "hypothesis refuted", "test before generalizing"),
                TestInstance.User,
                null
            );
            Assert.Equal(ConfidenceLevel.High, memory.Explain(id).Confidence.Level);
            memory.Invalidate(id, "new observation", "reviewer");
        }
        using SqliteStore reopened = fixture.Open(readOnly: true);
        ClaimExplanation explanation = fixture.Memory(reopened).Explain(id);
        Assert.Equal(KnowledgeStatus.Invalidated, explanation.Claim.Status);
        Assert.Equal(ConfidenceLevel.Low, explanation.Confidence.Level);
        Assert.Equal(2, explanation.History.Count);
        Assert.Equal(
            KnowledgeStatus.Proposed,
            RecordJson.Read<MemoryRecord>(explanation.History[1].PreviousState!).Status
        );
        Assert.Single(fixture.Memory(reopened).List(RecordKind.Retex));
    }

    [Fact]
    public void CrossClientIdsEvidenceHistoryCountsAndExportsAreIsolated()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "client-a");
        string b = fixture.Client(store, "client-b");
        MemoryService memoryA = fixture.Memory(store, a);
        MemoryService memoryB = fixture.Memory(store, b);
        MemoryRecord claim = memoryA.CreateClaim(
            TestInstance.Fact("confidential-a"),
            TestInstance.User,
            null
        );
        MemoryRecord evidence = memoryA.CreateEvidence(
            new(claim.Id, EvidenceKind.SourceCode, "private-a", "lineage-a"),
            TestInstance.User,
            null
        );
        Assert.Empty(memoryB.List());
        Assert.Empty(fixture.Memory(store).List());
        Assert.Equal(
            "not_found",
            Assert.Throws<SermofurException>(() => memoryB.Get(claim.Id)).Code
        );
        Assert.Equal(
            "not_found",
            Assert.Throws<SermofurException>(() => memoryB.Get(evidence.Id)).Code
        );
        Assert.Equal(
            "not_found",
            Assert
                .Throws<SermofurException>(() =>
                    memoryB.CreateEvidence(
                        new(claim.Id, EvidenceKind.SourceCode, "copy", "root"),
                        TestInstance.User,
                        null
                    )
                )
                .Code
        );
        Assert.Empty(memoryB.Export());
        Assert.Empty(store.ReadHistory(claim.Id, new HashSet<string> { "client-b", "workspace" }));
    }

    [Fact]
    public void ChildCanReadParentButCannotMutateOrAttachProof()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryRecord claim = fixture
            .Memory(store)
            .CreateClaim(TestInstance.Fact(), TestInstance.User, null);
        string path = fixture.Client(store, "child");
        MemoryService memory = fixture.Memory(store, path);
        Assert.Equal(claim.Id, memory.Get(claim.Id).Id);
        Assert.Equal(
            "scope_boundary",
            Assert
                .Throws<SermofurException>(() => memory.Invalidate(claim.Id, "reason", "actor"))
                .Code
        );
        Assert.Equal(
            "scope_boundary",
            Assert
                .Throws<SermofurException>(() =>
                    memory.CreateEvidence(
                        new(claim.Id, EvidenceKind.SourceCode, "ref", "root"),
                        TestInstance.User,
                        null
                    )
                )
                .Code
        );
    }

    [Fact]
    public async Task ConcurrentWritesAreCompleteAndIdempotent()
    {
        using TestInstance fixture = new TestInstance();
        await Task.WhenAll(
            Enumerable
                .Range(0, 20)
                .Select(index =>
                    Task.Run(() =>
                    {
                        using SqliteStore store = fixture.Open();
                        fixture
                            .Memory(store)
                            .CreateClaim(
                                TestInstance.Fact("claim " + index),
                                TestInstance.User,
                                "key-" + index
                            );
                    })
                )
        );
        using SqliteStore read = fixture.Open(true);
        Assert.Equal(20, fixture.Memory(read).List().Count);
        Assert.All(
            fixture.Memory(read).List(),
            record => Assert.Single(fixture.Memory(read).Explain(record.Id).History)
        );
        Assert.Equal("healthy_with_warnings", new InstanceDoctor().Inspect(fixture.Root).Overall);
    }

    [Fact]
    public void ProjectionLossDoesNotLoseCanonicalDataAndExportRepairs()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryService memory = fixture.Memory(store);
        MemoryRecord record = memory.CreateClaim(TestInstance.Fact(), TestInstance.User, null);
        File.Delete(new MarkdownProjection(fixture.Root).PathFor(record));
        Assert.Equal(record, memory.Get(record.Id));
        Assert.Equal(
            "warning",
            new InstanceDoctor()
                .Inspect(fixture.Root)
                .Checks.Single(c => c.Name == "projections")
                .Status
        );
        memory.Export();
        Assert.True(new MarkdownProjection(fixture.Root).IsCurrent(record));
    }

    [Fact]
    public void InvalidInputsAndLlmPreferencesDoNotCreateRecords()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryService memory = fixture.Memory(store);
        Assert.Throws<SermofurException>(() =>
            memory.CreateClaim(TestInstance.Fact(" "), TestInstance.User, null)
        );
        Assert.Throws<SermofurException>(() =>
            memory.CreateRetex(new("event", "", "next"), TestInstance.User, null)
        );
        Assert.Throws<SermofurException>(() =>
            memory.CreateClaim(
                new("always", MemoryCategory.Preferences, Volatility.Stable),
                TestInstance.User with
                {
                    Origin = ActorKind.Llm,
                },
                null
            )
        );
        Assert.Empty(memory.List());
    }
}
