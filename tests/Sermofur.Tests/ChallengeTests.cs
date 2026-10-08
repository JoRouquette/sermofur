using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class ChallengeTests
{
    [Fact]
    public void IndependentContradictionIsSignaledAndCapsConfidence()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryService memory = fixture.Memory(store);
        MemoryRecord claim = memory.CreateClaim(
            TestInstance.Fact("le build passe en 3 min"),
            TestInstance.User,
            null
        );
        memory.CreateEvidence(
            new(claim.Id, EvidenceKind.SourceCode, "pipeline.yml", "ci"),
            TestInstance.User,
            null
        );
        Assert.Equal(ConfidenceLevel.High, memory.Explain(claim.Id).Confidence.Level);

        MemoryRecord contra = memory.CreateEvidence(
            new(
                claim.Id,
                EvidenceKind.HumanObservation,
                "run 42 : 9 min",
                "observation",
                EvidenceRelation.Contradicts
            ),
            TestInstance.User,
            null
        );
        Confidence confidence = memory.Explain(claim.Id).Confidence;
        Assert.Equal(ConfidenceLevel.Medium, confidence.Level);
        Assert.Contains(
            confidence.Reasons,
            reason => reason.StartsWith("Contested by 1", StringComparison.Ordinal)
        );
        Assert.Equal(1, confidence.IndependentOrigins);

        ChallengeSignal signal = Assert.Single(
            fixture.Challenge(store).Challenge(claim.Id).Signals
        );
        Assert.Equal("contradiction", signal.Type);
        Assert.Equal(contra.Id, signal.EvidenceId);
        Assert.Contains("run 42", signal.Explanation);
    }

    [Fact]
    public void LlmContradictionIsNotedWithoutRefuting()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryService memory = fixture.Memory(store);
        MemoryRecord claim = memory.CreateClaim(
            TestInstance.Fact("l'API répond en JSON"),
            TestInstance.User,
            null
        );
        memory.CreateEvidence(
            new(claim.Id, EvidenceKind.SourceCode, "api.cs", "code"),
            TestInstance.User,
            null
        );
        Provenance llm = new(ActorKind.Llm, "assistant", DateTimeOffset.UtcNow);
        memory.CreateEvidence(
            new(
                claim.Id,
                EvidenceKind.LlmAssertion,
                "elle répond en XML",
                "llm",
                EvidenceRelation.Contradicts
            ),
            llm,
            null
        );
        Confidence confidence = memory.Explain(claim.Id).Confidence;
        Assert.Equal(ConfidenceLevel.High, confidence.Level);
        Assert.Contains(
            confidence.Reasons,
            reason => reason.Contains("not an independent refutation")
        );
        Assert.Contains(
            "not an independent refutation",
            Assert.Single(fixture.Challenge(store).Challenge(claim.Id).Signals).Explanation
        );
    }

    [Fact]
    public void FormatOneEvidenceReadsAsSupportWithoutSource()
    {
        EvidenceContent legacy = RecordJson.Read<EvidenceContent>(
            """{"claimId":"7f8c1a3e-0000-4000-8000-000000000001","kind":"source_code","reference":"r","lineageId":"l"}"""
        );
        Assert.Equal(EvidenceRelation.Supports, legacy.Relation);
        Assert.Null(legacy.SourceId);
        Assert.Null(legacy.SourceHash);
    }

    [Fact]
    public void ChangedAndUnavailableSourcesAreSignaledWithTheirHashes()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string file = fixture.WriteFile("adr.md", "la base est postgres");
        MemoryRecord source = fixture.Sources(store).Add(file, TestInstance.User);
        MemoryService memory = fixture.Memory(store);
        MemoryRecord claim = memory.CreateClaim(
            TestInstance.Fact("la base est postgres"),
            TestInstance.User,
            null
        );
        memory.CreateEvidence(
            new(claim.Id, EvidenceKind.ProjectDecision, "adr.md", "adr", SourceId: source.Id),
            TestInstance.User,
            null
        );
        Assert.Empty(fixture.Challenge(store).Challenge(claim.Id).Signals);

        File.WriteAllText(file, "la base est mysql");
        fixture.Sources(store).Reindex(null, "tester");
        ChallengeSignal changed = Assert.Single(
            fixture.Challenge(store).Challenge(claim.Id).Signals
        );
        Assert.Equal("source_changed", changed.Type);
        Assert.Equal(SourceService.Content(source).Hash, changed.RecordedHash);
        Assert.NotEqual(changed.RecordedHash, changed.CurrentHash);

        File.Delete(file);
        fixture.Sources(store).Reindex(null, "tester");
        Assert.Equal(
            "source_unavailable",
            Assert.Single(fixture.Challenge(store).Challenge(claim.Id).Signals).Type
        );
    }

    [Fact]
    public void EvidenceCitingASourceMovedToANarrowerScopeIsUnavailableWithoutItsPath()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryRecord source = fixture
            .Sources(store)
            .Add(fixture.WriteFile("a/plan.md", "le plan de a"), TestInstance.User);
        MemoryService memory = fixture.Memory(store);
        MemoryRecord claim = memory.CreateClaim(
            TestInstance.Fact("le plan existe"),
            TestInstance.User,
            null
        );
        memory.CreateEvidence(
            new(claim.Id, EvidenceKind.ProjectDecision, "plan", "plan", SourceId: source.Id),
            TestInstance.User,
            null
        );
        fixture.Client(store, "a");

        ChallengeSignal signal = Assert.Single(
            fixture.Challenge(store).Challenge(claim.Id).Signals
        );
        Assert.Equal("source_unavailable", signal.Type);
        Assert.Contains("no longer visible", signal.Explanation);
        Assert.DoesNotContain("plan.md", signal.Explanation);
    }

    [Fact]
    public void StatusAndReviewDateAreSignaled()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryService memory = fixture.Memory(store);
        MemoryRecord old = memory.CreateClaim(
            TestInstance.Fact("ancien fait"),
            TestInstance.User,
            null
        );
        memory.Invalidate(old.Id, "remplacé par la v2", "tester");
        ChallengeSignal invalidated = Assert.Single(
            fixture.Challenge(store).Challenge(old.Id).Signals
        );
        Assert.Equal("not_applicable", invalidated.Type);
        Assert.Contains("remplacé par la v2", invalidated.Explanation);

        MemoryRecord due = memory.CreateClaim(
            new ClaimContent(
                "fait à revoir",
                MemoryCategory.Semantic,
                Volatility.Volatile,
                ReviewAfter: DateTimeOffset.UtcNow.AddDays(-1)
            ),
            TestInstance.User,
            null
        );
        Assert.Equal(
            "review_due",
            Assert.Single(fixture.Challenge(store).Challenge(due.Id).Signals).Type
        );
    }

    [Fact]
    public void CloseClaimsAreToConfrontNeverContradictionsAndNothingIsWritten()
    {
        using TestInstance fixture = new TestInstance();
        MemoryRecord claim;
        using (SqliteStore store = fixture.Open())
        {
            MemoryService memory = fixture.Memory(store);
            claim = memory.CreateClaim(
                TestInstance.Fact("le cache redis expire après une heure"),
                TestInstance.User,
                null
            );
            for (int index = 0; index < 5; index++)
            {
                memory.CreateClaim(
                    TestInstance.Fact($"redis expire {index}"),
                    TestInstance.User,
                    null
                );
            }
        }
        Dictionary<string, string> before = fixture.Snapshot();
        using (SqliteStore store = fixture.Open(readOnly: true))
        {
            ChallengeAnswer answer = fixture.Challenge(store).Challenge(claim.Id);
            Assert.Empty(answer.Signals);
            Assert.Equal(3, answer.ToConfront.Count);
            Assert.DoesNotContain(answer.ToConfront, result => result.Id == claim.Id);
            Assert.All(answer.ToConfront, result => Assert.Equal("claim", result.Kind));
            ChallengeAnswer text = fixture.Challenge(store).Challenge("redis expire vite");
            Assert.Null(text.Target.ClaimId);
            Assert.Equal(3, text.ToConfront.Count);
        }
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void ClaimOfAnotherClientAnswersLikeAnUnknownIdentifier()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        string b = fixture.Client(store, "b");
        MemoryRecord hidden = fixture
            .Memory(store, a)
            .CreateClaim(TestInstance.Fact("secret de a"), TestInstance.User, null);
        SermofurException sibling = Assert.Throws<SermofurException>(() =>
            fixture.Challenge(store, b).Challenge(hidden.Id)
        );
        SermofurException unknown = Assert.Throws<SermofurException>(() =>
            fixture.Challenge(store, b).Challenge(Guid.NewGuid())
        );
        Assert.Equal((unknown.Code, unknown.Message), (sibling.Code, sibling.Message));
        Assert.Empty(fixture.Challenge(store, b).Challenge("secret").ToConfront);
    }

    [Fact]
    public void ChallengeAndEvidenceOptionsWorkThroughTheCli()
    {
        using TestInstance fixture = new TestInstance();
        CliResult claim = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "claim",
            "add",
            "le port est 8080",
            "--origin",
            "user"
        );
        string id = System
            .Text.Json.JsonDocument.Parse(claim.Output)
            .RootElement.GetProperty("id")
            .GetString()!;
        CliResult contra = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "evidence",
            "add",
            id,
            "human_observation",
            "netstat",
            "--lineage",
            "obs",
            "--origin",
            "user",
            "--contradicts"
        );
        Assert.Equal(0, contra.ExitCode);
        Assert.Contains("contradicts", contra.Output);
        CliResult challenge = TestInstance.Run("--path", fixture.Root, "--json", "challenge", id);
        Assert.Contains("\"type\": \"contradiction\"", challenge.Output);
        CliResult text = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "challenge",
            "--text",
            "port 8080"
        );
        Assert.Equal(0, text.ExitCode);
    }
}
