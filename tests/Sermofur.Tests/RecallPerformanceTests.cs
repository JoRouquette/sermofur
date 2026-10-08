using System.Diagnostics;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;
using Xunit.Abstractions;

namespace Sermofur.Tests;

/// <summary>
/// Reference measure of SC-004: 10,000 claims and RETEX (half of the claims with evidence, a
/// quarter of it contradicting) plus 1,000 sources of 20 KiB, recall p95 measured over 30
/// questions, each on a newly opened store, as a CLI command does. The data is inserted in bulk through the same index writer
/// as the store, without projections, to keep the setup short.
/// </summary>
public class RecallPerformanceTests(ITestOutputHelper output)
{
    private static readonly string[] Vocabulary =
    [
        "cache",
        "déploiement",
        "postgres",
        "migration",
        "index",
        "latence",
        "réseau",
        "build",
        "pipeline",
        "certificat",
        "keycloak",
        "angular",
        "service",
        "requête",
        "journal",
        "sauvegarde",
        "restauration",
        "quota",
        "mémoire",
        "processus",
        "thread",
        "verrou",
        "transaction",
        "schéma",
    ];

    [PerformanceFact]
    [Trait("Category", "Performance")]
    public void RecallStaysUnderTheReferenceBudgetOnALargeInstance()
    {
        using TestInstance fixture = new TestInstance();
        Seed(fixture.Root);
        Random random = new Random(42);
        List<double> timings = new List<double>();
        for (int index = 0; index < 30; index++)
        {
            string question = $"{Pick(random)} {Pick(random)} {Pick(random)}";
            Stopwatch watch = Stopwatch.StartNew();
            using SqliteStore store = fixture.Open(readOnly: true);
            RecallAnswer answer = fixture.Recall(store).Recall(question);
            watch.Stop();
            timings.Add(watch.Elapsed.TotalMilliseconds);
            Assert.InRange(answer.Results.Count, 1, RecallService.MaxResults);
        }
        timings.Sort();
        double p95 = timings[(int)Math.Ceiling(timings.Count * 0.95) - 1];
        output.WriteLine(
            $"recall p50 {timings[timings.Count / 2]:F0} ms, p95 {p95:F0} ms, max {timings[^1]:F0} ms"
        );
        Assert.True(p95 < 1_000, $"recall p95 {p95:F0} ms exceeds the 1 s ceiling of SC-004");
    }

    private static string Pick(Random random) => Vocabulary[random.Next(Vocabulary.Length)];

    private static void Seed(string root)
    {
        Random random = new Random(7);
        using SqliteConnection connection = new(
            $"Pooling=False;Data Source={InstanceManager.DatabasePath(root)}"
        );
        connection.Open();
        using SqliteTransaction transaction = connection.BeginTransaction();
        Provenance user = TestInstance.User;
        for (int index = 0; index < 10_000; index++)
        {
            bool claim = index % 5 != 0;
            string text = Sentence(random, 12);
            MemoryRecord record = new MemoryRecord(
                Guid.NewGuid(),
                "workspace",
                claim ? RecordKind.Claim : RecordKind.Retex,
                claim ? KnowledgeStatus.Proposed : KnowledgeStatus.Draft,
                1,
                claim
                    ? RecordJson.Write(TestInstance.Fact(text))
                    : RecordJson.Write(new RetexContent(text, "impact", "next")),
                user
            );
            Insert(connection, transaction, record);
            if (claim && index % 2 == 1)
            {
                EvidenceRelation relation =
                    index % 8 == 1 ? EvidenceRelation.Contradicts : EvidenceRelation.Supports;
                MemoryRecord evidence = new MemoryRecord(
                    Guid.NewGuid(),
                    "workspace",
                    RecordKind.Evidence,
                    KnowledgeStatus.Proposed,
                    1,
                    RecordJson.Write(
                        new EvidenceContent(
                            record.Id,
                            EvidenceKind.SourceCode,
                            "src/file.cs",
                            $"origin{index % 50}",
                            relation
                        )
                    ),
                    user
                );
                Insert(connection, transaction, evidence, claimId: record.Id);
            }
            SqliteSearchIndex.Insert(
                connection,
                transaction,
                record.Id,
                record.ScopeId,
                record.Kind,
                SearchDocuments.For(record)
            );
        }
        for (int index = 0; index < 1_000; index++)
        {
            string text = string.Join(
                "\n\n",
                Enumerable.Range(0, 20).Select(_ => Sentence(random, 140))
            );
            IReadOnlyList<SearchDocument> passages = SourcePassages.Split(text);
            SourceContent content = new(
                $"docs/{index}.md",
                SourceStatus.Indexed,
                "hash",
                text.Length,
                DateTimeOffset.UtcNow,
                passages.Count
            );
            MemoryRecord record = new MemoryRecord(
                Guid.NewGuid(),
                "workspace",
                RecordKind.Source,
                KnowledgeStatus.Active,
                1,
                RecordJson.Write(content),
                user
            );
            Insert(connection, transaction, record, content.RelativePath);
            SqliteSearchIndex.Insert(
                connection,
                transaction,
                record.Id,
                record.ScopeId,
                RecordKind.Source,
                passages
            );
        }
        transaction.Commit();
    }

    private static string Sentence(Random random, int words) =>
        string.Join(
            ' ',
            Enumerable
                .Range(0, words)
                .Select(_ =>
                    random.Next(4) == 0
                        ? Vocabulary[random.Next(Vocabulary.Length)]
                        : $"mot{random.Next(5_000)}"
                )
        );

    private static void Insert(
        SqliteConnection connection,
        SqliteTransaction transaction,
        MemoryRecord record,
        string? path = null,
        Guid? claimId = null
    )
    {
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            "INSERT INTO records(id,scope_id,kind,claim_id,payload,revision,source_path) VALUES($id,$scope,$kind,$claim,$payload,1,$path)";
        command.Parameters.AddWithValue("$id", record.Id.ToString());
        command.Parameters.AddWithValue("$scope", record.ScopeId);
        command.Parameters.AddWithValue("$kind", record.Kind.ToString());
        command.Parameters.AddWithValue("$payload", RecordJson.Write(record));
        command.Parameters.AddWithValue("$path", (object?)path ?? DBNull.Value);
        command.Parameters.AddWithValue("$claim", (object?)claimId?.ToString() ?? DBNull.Value);
        command.ExecuteNonQuery();
    }
}
