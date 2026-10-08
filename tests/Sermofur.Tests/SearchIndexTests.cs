using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class SearchIndexTests
{
    [Fact]
    public void FullTextSearchIsAvailableAndClaimsAreIndexedOnCreation()
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        Assert.Equal(1L, store.Scalar("SELECT sqlite_compileoption_used('ENABLE_FTS5')"));
        MemoryService memory = fixture.Memory(store);
        MemoryRecord claim = memory.CreateClaim(
            TestInstance.Fact("Déploiement à chaud"),
            TestInstance.User,
            null
        );
        memory.CreateRetex(new("event", "impact", "next"), TestInstance.User, null);
        Assert.Equal(
            claim.Id.ToString(),
            store.Scalar("SELECT object_id FROM search WHERE search MATCH '\"deploiement\"'")
        );
        Assert.Equal(2L, store.Scalar("SELECT count(*) FROM search"));
    }

    [Theory]
    [InlineData("Élan, CAFÉ-crème; naïve Straße 42 ½ x_y ß")]
    [InlineData("日本語のテキスト mixed with ASCII")]
    [InlineData("emoji 🙂 between words")]
    [InlineData("tabs\tand\nlines\r\nend")]
    public void QueryTokensMatchTheTokensOfTheIndex(string text)
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        fixture.Memory(store).CreateClaim(TestInstance.Fact(text), TestInstance.User, null);
        IReadOnlyList<string> indexed = store.QueryStrings(
            "SELECT term FROM search_terms WHERE col='text' ORDER BY offset"
        );
        Assert.Equal(indexed, store.Tokenizer.Tokenize(text));
    }

    [Theory]
    [InlineData("\"")]
    [InlineData("a\" OR \"b")]
    [InlineData("NEAR(a b) AND col:x*")]
    [InlineData("^start - not")]
    public void MatchExpressionNeverCarriesQuerySyntax(string query)
    {
        using TestInstance fixture = new TestInstance();
        using SqliteStore store = fixture.Open();
        IReadOnlyList<string> terms = SearchTerms.Query(store.Tokenizer, query);
        if (terms.Count == 0)
        {
            return;
        }
        Assert.All(terms, term => Assert.Matches(@"^[\p{L}\p{N}\p{Co}]+$", term));
        using SqliteConnection connection = new(
            $"Pooling=False;Data Source={InstanceManager.DatabasePath(fixture.Root)}"
        );
        connection.Open();
        using SqliteCommand command = connection.CreateCommand();
        command.CommandText = "SELECT count(*) FROM search WHERE search MATCH $q";
        command.Parameters.AddWithValue("$q", SearchTerms.MatchExpression(terms));
        Assert.Equal(0L, command.ExecuteScalar());
    }

    [Fact]
    public void QueryWithoutTermsIsAnInputError()
    {
        Assert.Equal(
            "invalid_input",
            Assert.Throws<SermofurException>(() => SearchTerms.MatchExpression([])).Code
        );
    }
}
