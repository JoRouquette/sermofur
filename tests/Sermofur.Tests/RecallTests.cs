using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class RecallTests
{
    [Fact]
    public void RecallReturnsAtMostThreeExplainedResultsInAStableOrder()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryService memory = fixture.Memory(store);
        for (int index = 0; index < 5; index++)
        {
            memory.CreateClaim(
                TestInstance.Fact($"le cache {index} se vide au déploiement"),
                TestInstance.User,
                null
            );
        }
        memory.CreateRetex(
            new("Cache vidé", "Lenteur", "Préchauffer le cache"),
            TestInstance.User,
            null
        );

        RecallAnswer answer = fixture.Recall(store).Recall("CACHE déploiement");
        Assert.Equal(3, answer.Results.Count);
        Assert.All(
            answer.Results,
            result =>
            {
                Assert.Equal("workspace", result.ScopeId);
                Assert.True(result.Applicable);
                Assert.NotEmpty(result.Excerpt);
                Assert.NotEmpty(result.MatchedTerms);
            }
        );
        Assert.All(
            answer.Results.Where(r => r.Kind == "claim"),
            result => Assert.NotNull(result.Confidence)
        );
        Assert.Equal(
            answer.Results.Select(r => r.Id),
            fixture.Recall(store).Recall("CACHE déploiement").Results.Select(r => r.Id)
        );
        Assert.Single(fixture.Recall(store).Recall("cache", limit: 1).Results);
    }

    [Fact]
    public void ContentOfASiblingNeverChangesPresenceOrderOrScoreOfVisibleResults()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        string b = fixture.Client(store, "b");
        MemoryService inA = fixture.Memory(store, a);
        inA.CreateClaim(
            TestInstance.Fact("migration de la base postgres"),
            TestInstance.User,
            null
        );
        inA.CreateClaim(
            TestInstance.Fact("postgres tourne en version 16"),
            TestInstance.User,
            null
        );
        inA.CreateClaim(TestInstance.Fact("la migration se fait la nuit"), TestInstance.User, null);
        RecallAnswer before = fixture.Recall(store, a).Recall("migration postgres");

        // Client b gets far more relevant content, which would change global BM25 statistics.
        MemoryService inB = fixture.Memory(store, b);
        for (int index = 0; index < 20; index++)
        {
            inB.CreateClaim(
                TestInstance.Fact("postgres postgres postgres migration"),
                TestInstance.User,
                null
            );
        }
        fixture
            .Sources(store, b)
            .Add(
                fixture.WriteFile("b/doc.md", "migration postgres migration postgres"),
                TestInstance.User
            );

        RecallAnswer after = fixture.Recall(store, a).Recall("migration postgres");
        Assert.Equal(before, after, new RecallAnswerComparer());
        Assert.All(after.Results, result => Assert.Equal("a", result.ScopeId));
    }

    [Fact]
    public void NonApplicableKnowledgeIsNeverPresentedAsAFact()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        MemoryService memory = fixture.Memory(store);
        MemoryRecord stale = memory.CreateClaim(
            TestInstance.Fact("redis redis redis redis cache"),
            TestInstance.User,
            null
        );
        memory.Invalidate(stale.Id, "redis retiré", "tester");
        MemoryRecord current = memory.CreateClaim(
            TestInstance.Fact("le cache utilise valkey, pas redis"),
            TestInstance.User,
            null
        );

        RecallAnswer answer = fixture.Recall(store).Recall("redis");
        Assert.Equal(current.Id, answer.Results[0].Id);
        RecallResult invalidated = answer.Results.Single(r => r.Id == stale.Id);
        Assert.False(invalidated.Applicable);
        Assert.Equal("invalidated", invalidated.Status);

        RecallAnswer one = fixture.Recall(store).Recall("redis", limit: 1);
        Assert.Equal(current.Id, Assert.Single(one.Results).Id);
        Assert.Equal(1, one.Excluded);
    }

    [Fact]
    public void SourcesComeBackWithTheirBestPassageAndFreshness()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        string filler = string.Join("\n\n", Enumerable.Repeat(new string('x', 1500), 3));
        string file = fixture.WriteFile(
            "guide.md",
            filler + "\n\nLe préchauffage du cache prend dix minutes.\n\n" + filler
        );
        MemoryRecord source = fixture.Sources(store).Add(file, TestInstance.User);

        RecallResult result = Assert.Single(fixture.Recall(store).Recall("prechauffage").Results);
        Assert.Equal("source", result.Kind);
        Assert.Equal("guide.md", result.Path);
        Assert.Contains("préchauffage du cache", result.Excerpt);
        Assert.Equal(SourceService.Content(source).Hash, result.Freshness.Hash);
        Assert.NotNull(result.Freshness.IndexedAt);
    }

    [Theory]
    [InlineData("\"cache\" OR *")]
    [InlineData("NEAR(cache, x) AND col:text")]
    public void QuestionSyntaxIsPlainText(string question)
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        fixture
            .Memory(store)
            .CreateClaim(TestInstance.Fact("cache chaud"), TestInstance.User, null);
        Assert.Single(fixture.Recall(store).Recall(question).Results);
        Assert.Empty(fixture.Recall(store).Recall("introuvable").Results);
    }

    [Theory]
    [InlineData("0")]
    [InlineData("4")]
    [InlineData("x")]
    public void LimitIsBetweenOneAndThree(string limit)
    {
        using TestInstance fixture = new TestInstance();
        CliResult result = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "recall",
            "cache",
            "--limit",
            limit
        );
        Assert.Equal("invalid_arguments", TestInstance.ErrorCode(result));
    }

    [Fact]
    public async Task RecallRunsOnTheReadOnlyStoreOfARealProcess()
    {
        using TestInstance fixture = new TestInstance();
        using (SqliteStore store = fixture.Open())
        {
            fixture
                .Memory(store)
                .CreateClaim(TestInstance.Fact("déploiement le mardi"), TestInstance.User, null);
        }
        Dictionary<string, string> before = fixture.Snapshot();
        CliResult result = await TestInstance.RunCli(
            "--path",
            fixture.Root,
            "--json",
            "recall",
            "deploiement"
        );
        Assert.Equal(0, result.ExitCode);
        Assert.Contains("déploiement le mardi", result.Output);
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    private sealed class RecallAnswerComparer : IEqualityComparer<RecallAnswer>
    {
        public bool Equals(RecallAnswer? x, RecallAnswer? y) =>
            x is not null
            && y is not null
            && x.Excluded == y.Excluded
            && x.Results.Select(Key).SequenceEqual(y.Results.Select(Key));

        public int GetHashCode(RecallAnswer obj) => obj.Results.Count;

        private static string Key(RecallResult result) =>
            $"{result.Id}|{result.Score}|{result.Excerpt}|{string.Join(',', result.MatchedTerms)}";
    }
}
