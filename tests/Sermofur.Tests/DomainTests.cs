using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class DomainTests
{
    [Fact]
    public void VisibilityIncludesOnlyAncestors()
    {
        Scope[] scopes =
        [
            new("workspace", ScopeKind.Workspace, null, null),
            new("a", ScopeKind.Client, "workspace", "a"),
            new("b", ScopeKind.Client, "workspace", "b"),
            new("ap", ScopeKind.Project, "a", "a/project"),
        ];
        ScopePolicy.ValidateTree(scopes);
        Assert.Equal(
            new[] { "a", "ap", "workspace" },
            ScopePolicy.VisibleAncestors("ap", scopes).Order().ToArray()
        );
        Assert.Equal(
            new[] { "workspace" },
            ScopePolicy.VisibleAncestors("workspace", scopes).ToArray()
        );
    }

    [Fact]
    public void CyclesAndInvalidParentsFailClosed()
    {
        Assert.Throws<SermofurException>(() =>
            ScopePolicy.VisibleAncestors(
                "a",
                [new("a", ScopeKind.Client, "b", "a"), new("b", ScopeKind.Project, "a", "b")]
            )
        );
        Assert.Throws<SermofurException>(() =>
            ScopePolicy.ValidateTree([
                new("workspace", ScopeKind.Workspace, null, null),
                new("repo", ScopeKind.Repository, "workspace", "repo"),
            ])
        );
    }

    [Fact]
    public void RepeatedLlmAssertionsRemainLowAndOneOrigin()
    {
        MemoryRecord claim = new MemoryRecord(
            Guid.NewGuid(),
            "workspace",
            RecordKind.Claim,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(TestInstance.Fact()),
            TestInstance.User
        );
        MemoryRecord[] evidence = Enumerable
            .Range(0, 12)
            .Select(_ => new MemoryRecord(
                Guid.NewGuid(),
                "workspace",
                RecordKind.Evidence,
                KnowledgeStatus.Proposed,
                1,
                RecordJson.Write(
                    new EvidenceContent(
                        claim.Id,
                        EvidenceKind.LlmAssertion,
                        "repetition",
                        "same-root"
                    )
                ),
                TestInstance.User
            ))
            .ToArray();
        Confidence confidence = MemoryService.EvaluateConfidence(claim, evidence);
        Assert.Equal(ConfidenceLevel.Low, confidence.Level);
        Assert.Equal(1, confidence.IndependentOrigins);
    }

    [Theory]
    [InlineData(EvidenceKind.Execution, ConfidenceLevel.Low)]
    [InlineData(EvidenceKind.UserAssertion, ConfidenceLevel.Low)]
    [InlineData(EvidenceKind.SourceCode, ConfidenceLevel.High)]
    [InlineData(EvidenceKind.AuthoritativeDocumentation, ConfidenceLevel.High)]
    public void DeclaredExecutionCannotSelfVerify(EvidenceKind kind, ConfidenceLevel expected)
    {
        MemoryRecord claim = new MemoryRecord(
            Guid.NewGuid(),
            "workspace",
            RecordKind.Claim,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(TestInstance.Fact()),
            TestInstance.User
        );
        MemoryRecord evidence = claim with
        {
            Id = Guid.NewGuid(),
            Kind = RecordKind.Evidence,
            ContentJson = RecordJson.Write(new EvidenceContent(claim.Id, kind, "source", "root")),
        };
        Assert.Equal(expected, MemoryService.EvaluateConfidence(claim, [evidence]).Level);
    }

    [Fact]
    public void GeneratedTreesNeverRevealSiblingsOrDescendants()
    {
        Random random = new Random(42);
        for (int sample = 0; sample < 100; sample++)
        {
            int count = random.Next(2, 30);
            List<Scope> scopes = new List<Scope>
            {
                new("workspace", ScopeKind.Workspace, null, null),
            };
            for (int i = 0; i < count; i++)
            {
                scopes.Add(new("client-" + i, ScopeKind.Client, "workspace", "client-" + i));
            }

            ScopePolicy.ValidateTree(scopes);
            foreach (Scope scope in scopes.Skip(1))
            {
                Assert.Equal(2, ScopePolicy.VisibleAncestors(scope.Id, scopes).Count);
            }
        }
    }

    [Fact]
    public void LlmCannotSelfLabelItsOutputAsObservedCodeToRaiseConfidence()
    {
        MemoryRecord claim = new MemoryRecord(
            Guid.NewGuid(),
            "workspace",
            RecordKind.Claim,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(TestInstance.Fact()),
            TestInstance.User
        );
        MemoryRecord evidence = claim with
        {
            Id = Guid.NewGuid(),
            Kind = RecordKind.Evidence,
            ContentJson = RecordJson.Write(
                new EvidenceContent(claim.Id, EvidenceKind.SourceCode, "pretended-code", "llm-root")
            ),
            Provenance = TestInstance.User with { Origin = ActorKind.Llm },
        };
        Assert.Equal(
            ConfidenceLevel.Low,
            MemoryService.EvaluateConfidence(claim, [evidence]).Level
        );
    }
}
