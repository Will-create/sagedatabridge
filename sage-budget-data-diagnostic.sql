/*
    Sage 1000 budget diagnostics for Sage Data Bridge
    -------------------------------------------------
    READ ONLY: this script only reads metadata/data and writes to a local
    temporary table (#diagnostic), which disappears when the session ends.

    Run this against the same Sage database whose schema was exported to
    sql-results-2026-06-22T17-03-42-804Z.csv. Export/copy the final result grid.
    The Details column contains JSON so that all evidence stays in one grid.
*/

SET NOCOUNT ON;
SET TRANSACTION ISOLATION LEVEL READ UNCOMMITTED;

CREATE TABLE #diagnostic (
    SortOrder   int IDENTITY(1,1) NOT NULL,
    SectionName nvarchar(80) NOT NULL,
    ObjectName  nvarchar(256) NULL,
    RowCount    bigint NULL,
    Details     nvarchar(max) NULL
);

INSERT INTO #diagnostic (SectionName, ObjectName, Details)
VALUES (
    N'DATABASE',
    DB_NAME(),
    CONCAT(
        N'{"server":', QUOTENAME(CAST(SERVERPROPERTY('ServerName') AS nvarchar(256)), '"'),
        N',"database":', QUOTENAME(DB_NAME(), '"'),
        N',"generatedAt":', QUOTENAME(CONVERT(nvarchar(33), SYSDATETIMEOFFSET(), 127), '"'),
        N'}'
    )
);

/* Resolve Sage alias types such as TOID to their SQL Server base types. */
INSERT INTO #diagnostic (SectionName, ObjectName, Details)
SELECT
    N'USER_TYPE',
    CONCAT(SCHEMA_NAME(alias_type.schema_id), N'.', alias_type.name),
    (
        SELECT
            base_type.name AS baseType,
            alias_type.max_length AS maxLength,
            alias_type.precision AS [precision],
            alias_type.scale AS scale,
            alias_type.is_nullable AS nullable
        FOR JSON PATH, WITHOUT_ARRAY_WRAPPER
    )
FROM sys.types alias_type
INNER JOIN sys.types base_type
    ON base_type.user_type_id = alias_type.system_type_id
   AND base_type.user_type_id = base_type.system_type_id
WHERE alias_type.is_user_defined = 1;

DECLARE @tables TABLE (TableName sysname PRIMARY KEY);
INSERT INTO @tables (TableName)
VALUES
    (N'TBUDGET'),
    (N'TBUDGETEXERCICE'),
    (N'TEXERCICEBUDGETAIRE'),
    (N'THYPOTHESEBUDGETAIRE'),
    (N'TTYPEHYPOTHESE'),
    (N'TELEMENTBUDGETAIRE'),
    (N'TELEMENTBUDGETEXERCICE'),
    (N'TELEMENTBUDGETPERIODE'),
    (N'TPOSTE'),
    (N'TPOSTEPARAMETRE'),
    (N'TNATUREBUDGETAIRE'),
    (N'TPERIODE'),
    (N'TPERIMETREBUDGETAIRE'),
    (N'TREALISEBUDGET'),
    (N'TIMPORTDETAILBUDGET'),
    (N'TIMPORTHYPOTHESEBUDGET'),
    (N'TGROUPEBUDGETAIRE'),
    (N'THYPOTHESEBUDGETAIREGCF'),
    (N'TPLANPOSTEBUDGETAIRE'),
    (N'TPOSTEBUDGETAIREGCF'),
    (N'TCS_CUMULANALYTIQUE');

DECLARE @table sysname;
DECLARE @sql nvarchar(max);

DECLARE table_cursor CURSOR LOCAL FAST_FORWARD FOR
    SELECT TableName FROM @tables ORDER BY TableName;

OPEN table_cursor;
FETCH NEXT FROM table_cursor INTO @table;
WHILE @@FETCH_STATUS = 0
BEGIN
    IF OBJECT_ID(N'dbo.' + @table, N'U') IS NULL
    BEGIN
        INSERT INTO #diagnostic (SectionName, ObjectName, Details)
        VALUES (N'ROW_COUNT', N'dbo.' + @table, N'{"exists":false}');
    END
    ELSE
    BEGIN
        SET @sql = N'INSERT INTO #diagnostic (SectionName, ObjectName, RowCount, Details)
                     SELECT N''ROW_COUNT'', N''dbo.' + REPLACE(@table, '''', '''''') + N''',
                            COUNT_BIG(*), N''{"exists":true}''
                     FROM dbo.' + QUOTENAME(@table) + N';';
        EXEC sys.sp_executesql @sql;
    END;
    FETCH NEXT FROM table_cursor INTO @table;
END;
CLOSE table_cursor;
DEALLOCATE table_cursor;

/* Declared foreign keys involving the budget tables. */
INSERT INTO #diagnostic (SectionName, ObjectName, Details)
SELECT
    N'FOREIGN_KEY',
    fk.name,
    (
        SELECT
            OBJECT_SCHEMA_NAME(fkc.parent_object_id) AS childSchema,
            OBJECT_NAME(fkc.parent_object_id) AS childTable,
            child_col.name AS childColumn,
            OBJECT_SCHEMA_NAME(fkc.referenced_object_id) AS parentSchema,
            OBJECT_NAME(fkc.referenced_object_id) AS parentTable,
            parent_col.name AS parentColumn
        FROM sys.foreign_key_columns fkc
        INNER JOIN sys.columns child_col
            ON child_col.object_id = fkc.parent_object_id
           AND child_col.column_id = fkc.parent_column_id
        INNER JOIN sys.columns parent_col
            ON parent_col.object_id = fkc.referenced_object_id
           AND parent_col.column_id = fkc.referenced_column_id
        WHERE fkc.constraint_object_id = fk.object_id
        ORDER BY fkc.constraint_column_id
        FOR JSON PATH
    )
FROM sys.foreign_keys fk
WHERE EXISTS (
    SELECT 1
    FROM sys.foreign_key_columns fkc
    WHERE fkc.constraint_object_id = fk.object_id
      AND (
          OBJECT_NAME(fkc.parent_object_id) IN (SELECT TableName FROM @tables)
          OR OBJECT_NAME(fkc.referenced_object_id) IN (SELECT TableName FROM @tables)
      )
);

/* Index coverage on the key join/filter columns. */
INSERT INTO #diagnostic (SectionName, ObjectName, Details)
SELECT
    N'INDEX',
    CONCAT(OBJECT_SCHEMA_NAME(i.object_id), N'.', OBJECT_NAME(i.object_id), N'.', i.name),
    (
        SELECT
            col.name AS columnName,
            ic.key_ordinal AS keyOrdinal,
            ic.is_included_column AS included
        FROM sys.index_columns ic
        INNER JOIN sys.columns col
            ON col.object_id = ic.object_id
           AND col.column_id = ic.column_id
        WHERE ic.object_id = i.object_id
          AND ic.index_id = i.index_id
        ORDER BY ic.key_ordinal, ic.index_column_id
        FOR JSON PATH
    )
FROM sys.indexes i
WHERE OBJECT_NAME(i.object_id) IN (SELECT TableName FROM @tables)
  AND i.name IS NOT NULL;

/*
   Validate the expected logical relationships even when Sage has not declared
   SQL Server foreign-key constraints. OrphanCount excludes NULL child keys.
*/
DECLARE @relationships TABLE (
    ChildTable  sysname,
    ChildColumn sysname,
    ParentTable sysname,
    ParentColumn sysname
);

INSERT INTO @relationships (ChildTable, ChildColumn, ParentTable, ParentColumn)
VALUES
    (N'TBUDGET', N'oidPerimetreBudgetaire', N'TPERIMETREBUDGETAIRE', N'oid'),
    (N'TBUDGETEXERCICE', N'oidBudget', N'TBUDGET', N'oid'),
    (N'TBUDGETEXERCICE', N'oidExerciceBudgetaire', N'TEXERCICEBUDGETAIRE', N'oid'),
    (N'THYPOTHESEBUDGETAIRE', N'oidBudgetExercice', N'TBUDGETEXERCICE', N'oid'),
    (N'THYPOTHESEBUDGETAIRE', N'oidTypeHypothese', N'TTYPEHYPOTHESE', N'oid'),
    (N'TELEMENTBUDGETAIRE', N'oidHypotheseBudgetaire', N'THYPOTHESEBUDGETAIRE', N'oid'),
    (N'TELEMENTBUDGETAIRE', N'oidPosteBudgetaire', N'TPOSTE', N'oid'),
    (N'TELEMENTBUDGETEXERCICE', N'oidHypotheseBudgetaire', N'THYPOTHESEBUDGETAIRE', N'oid'),
    (N'TELEMENTBUDGETEXERCICE', N'oidPosteBudgetaire', N'TPOSTE', N'oid'),
    (N'TELEMENTBUDGETPERIODE', N'oidElementBudgetExercice', N'TELEMENTBUDGETEXERCICE', N'oid'),
    (N'TELEMENTBUDGETPERIODE', N'oidPeriode', N'TPERIODE', N'oid'),
    (N'TPOSTE', N'oidNatureBudgetaire', N'TNATUREBUDGETAIRE', N'oid'),
    (N'TREALISEBUDGET', N'oidPerimetreBudgetaire', N'TPERIMETREBUDGETAIRE', N'oid'),
    (N'TREALISEBUDGET', N'oidPeriode', N'TPERIODE', N'oid'),
    (N'TREALISEBUDGET', N'oidPosteBudgetaire', N'TPOSTE', N'oid');

DECLARE @child_table sysname;
DECLARE @child_column sysname;
DECLARE @parent_table sysname;
DECLARE @parent_column sysname;

DECLARE relationship_cursor CURSOR LOCAL FAST_FORWARD FOR
    SELECT ChildTable, ChildColumn, ParentTable, ParentColumn
    FROM @relationships;

OPEN relationship_cursor;
FETCH NEXT FROM relationship_cursor
INTO @child_table, @child_column, @parent_table, @parent_column;
WHILE @@FETCH_STATUS = 0
BEGIN
    IF OBJECT_ID(N'dbo.' + @child_table, N'U') IS NOT NULL
       AND OBJECT_ID(N'dbo.' + @parent_table, N'U') IS NOT NULL
       AND COL_LENGTH(N'dbo.' + @child_table, @child_column) IS NOT NULL
       AND COL_LENGTH(N'dbo.' + @parent_table, @parent_column) IS NOT NULL
    BEGIN
        SET @sql = N'INSERT INTO #diagnostic (SectionName, ObjectName, RowCount, Details)
                     SELECT N''RELATIONSHIP_CHECK'',
                            N''' + REPLACE(@child_table + N'.' + @child_column + N' -> ' + @parent_table + N'.' + @parent_column, '''', '''''') + N''',
                            COUNT_BIG(*),
                            CONCAT(N''{"nonNullChildKeys":'', COUNT_BIG(c.' + QUOTENAME(@child_column) + N'),
                                   N'',"matchedRows":'', SUM(CASE WHEN p.' + QUOTENAME(@parent_column) + N' IS NOT NULL THEN CONVERT(bigint, 1) ELSE CONVERT(bigint, 0) END),
                                   N'',"orphanRows":'', SUM(CASE WHEN c.' + QUOTENAME(@child_column) + N' IS NOT NULL AND p.' + QUOTENAME(@parent_column) + N' IS NULL THEN CONVERT(bigint, 1) ELSE CONVERT(bigint, 0) END),
                                   N''}'')
                     FROM dbo.' + QUOTENAME(@child_table) + N' c
                     LEFT JOIN dbo.' + QUOTENAME(@parent_table) + N' p
                       ON p.' + QUOTENAME(@parent_column) + N' = c.' + QUOTENAME(@child_column) + N';';
        EXEC sys.sp_executesql @sql;
    END;
    FETCH NEXT FROM relationship_cursor
    INTO @child_table, @child_column, @parent_table, @parent_column;
END;
CLOSE relationship_cursor;
DEALLOCATE relationship_cursor;

/* Catalog every configured exercise/hypothesis and its amount coverage. */
IF OBJECT_ID(N'dbo.TBUDGET', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TBUDGETEXERCICE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TEXERCICEBUDGETAIRE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.THYPOTHESEBUDGETAIRE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TELEMENTBUDGETEXERCICE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TELEMENTBUDGETPERIODE', N'U') IS NOT NULL
BEGIN
    ;WITH annual_summary AS (
        SELECT
            oidHypotheseBudgetaire,
            COUNT_BIG(*) AS annualElementCount,
            COUNT(DISTINCT oidPosteBudgetaire) AS distinctPostCount,
            SUM(CONVERT(decimal(38,4), montant)) AS annualAmount
        FROM dbo.TELEMENTBUDGETEXERCICE
        GROUP BY oidHypotheseBudgetaire
    ),
    period_summary AS (
        SELECT
            annual_element.oidHypotheseBudgetaire,
            COUNT_BIG(*) AS periodElementCount,
            SUM(CONVERT(decimal(38,4), period_element.montant)) AS periodAmount,
            MIN(period_def.dateDebut) AS firstPeriodStart,
            MAX(period_def.dateFin) AS lastPeriodEnd
        FROM dbo.TELEMENTBUDGETEXERCICE annual_element
        INNER JOIN dbo.TELEMENTBUDGETPERIODE period_element
            ON period_element.oidElementBudgetExercice = annual_element.oid
        LEFT JOIN dbo.TPERIODE period_def
            ON period_def.oid = period_element.oidPeriode
        GROUP BY annual_element.oidHypotheseBudgetaire
    )
    INSERT INTO #diagnostic (SectionName, ObjectName, Details)
    SELECT
        N'BUDGET_CATALOG',
        N'Exercises and hypotheses',
        (
            SELECT TOP (200)
                perimeter.code AS perimeterCode,
                perimeter.Caption AS perimeterCaption,
                budget.Caption AS budgetCaption,
                exercise.code AS exerciseCode,
                exercise.Caption AS exerciseCaption,
                exercise.dateDebut AS exerciseStart,
                exercise.dateFin AS exerciseEnd,
                hypothesis.Caption AS hypothesisCaption,
                hypothesis_type.Caption AS hypothesisType,
                annual_summary.annualElementCount,
                annual_summary.distinctPostCount,
                annual_summary.annualAmount,
                period_summary.periodElementCount,
                period_summary.periodAmount,
                period_summary.firstPeriodStart,
                period_summary.lastPeriodEnd
            FROM dbo.THYPOTHESEBUDGETAIRE hypothesis
            LEFT JOIN dbo.TTYPEHYPOTHESE hypothesis_type
                ON hypothesis_type.oid = hypothesis.oidTypeHypothese
            LEFT JOIN dbo.TBUDGETEXERCICE budget_exercise
                ON budget_exercise.oid = hypothesis.oidBudgetExercice
            LEFT JOIN dbo.TBUDGET budget
                ON budget.oid = budget_exercise.oidBudget
            LEFT JOIN dbo.TPERIMETREBUDGETAIRE perimeter
                ON perimeter.oid = budget.oidPerimetreBudgetaire
            LEFT JOIN dbo.TEXERCICEBUDGETAIRE exercise
                ON exercise.oid = budget_exercise.oidExerciceBudgetaire
            LEFT JOIN annual_summary
                ON annual_summary.oidHypotheseBudgetaire = hypothesis.oid
            LEFT JOIN period_summary
                ON period_summary.oidHypotheseBudgetaire = hypothesis.oid
            ORDER BY exercise.dateDebut DESC, perimeter.code, budget.Caption, hypothesis.Caption
            FOR JSON PATH, INCLUDE_NULL_VALUES
        );

    INSERT INTO #diagnostic (SectionName, ObjectName, Details)
    SELECT
        N'BUDGET_POST_PROFILE',
        N'Budget posts used by monthly elements',
        (
            SELECT TOP (500)
                post.code AS postCode,
                post.Caption AS postCaption,
                nature.code AS natureCode,
                nature.Caption AS natureCaption,
                nature.sens AS natureDirection,
                COUNT_BIG(*) AS periodElementCount,
                SUM(CONVERT(decimal(38,4), period_element.montant)) AS periodAmount,
                MIN(period_def.dateDebut) AS firstPeriodStart,
                MAX(period_def.dateFin) AS lastPeriodEnd
            FROM dbo.TELEMENTBUDGETPERIODE period_element
            INNER JOIN dbo.TELEMENTBUDGETEXERCICE annual_element
                ON annual_element.oid = period_element.oidElementBudgetExercice
            LEFT JOIN dbo.TPOSTE post
                ON post.oid = annual_element.oidPosteBudgetaire
            LEFT JOIN dbo.TNATUREBUDGETAIRE nature
                ON nature.oid = post.oidNatureBudgetaire
            LEFT JOIN dbo.TPERIODE period_def
                ON period_def.oid = period_element.oidPeriode
            GROUP BY post.code, post.Caption, nature.code, nature.Caption, nature.sens
            ORDER BY post.code, post.Caption
            FOR JSON PATH, INCLUDE_NULL_VALUES
        );
END;

/* Small value samples reveal the active exercise/hypothesis/post conventions. */
DECLARE @sample_tables TABLE (TableName sysname PRIMARY KEY);
INSERT INTO @sample_tables (TableName)
VALUES
    (N'TBUDGET'),
    (N'TBUDGETEXERCICE'),
    (N'TEXERCICEBUDGETAIRE'),
    (N'THYPOTHESEBUDGETAIRE'),
    (N'TTYPEHYPOTHESE'),
    (N'TELEMENTBUDGETEXERCICE'),
    (N'TELEMENTBUDGETPERIODE'),
    (N'TPOSTE'),
    (N'TNATUREBUDGETAIRE'),
    (N'TPERIODE'),
    (N'TPERIMETREBUDGETAIRE'),
    (N'TREALISEBUDGET'),
    (N'TIMPORTDETAILBUDGET');

DECLARE sample_cursor CURSOR LOCAL FAST_FORWARD FOR
    SELECT TableName FROM @sample_tables ORDER BY TableName;

OPEN sample_cursor;
FETCH NEXT FROM sample_cursor INTO @table;
WHILE @@FETCH_STATUS = 0
BEGIN
    IF OBJECT_ID(N'dbo.' + @table, N'U') IS NOT NULL
    BEGIN
        SET @sql = N'INSERT INTO #diagnostic (SectionName, ObjectName, Details)
                     SELECT N''SAMPLE_TOP_20'', N''dbo.' + REPLACE(@table, '''', '''''') + N''',
                            (SELECT TOP (20) * FROM dbo.' + QUOTENAME(@table) + N' ORDER BY UpdDate DESC FOR JSON PATH, INCLUDE_NULL_VALUES);';
        EXEC sys.sp_executesql @sql;
    END;
    FETCH NEXT FROM sample_cursor INTO @table;
END;
CLOSE sample_cursor;
DEALLOCATE sample_cursor;

/* One compact joined sample of the monthly budget path. */
IF OBJECT_ID(N'dbo.TELEMENTBUDGETPERIODE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TELEMENTBUDGETEXERCICE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.THYPOTHESEBUDGETAIRE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TBUDGETEXERCICE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TEXERCICEBUDGETAIRE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TPOSTE', N'U') IS NOT NULL
   AND OBJECT_ID(N'dbo.TPERIODE', N'U') IS NOT NULL
BEGIN
    INSERT INTO #diagnostic (SectionName, ObjectName, Details)
    SELECT
        N'JOINED_SAMPLE',
        N'Monthly budget path',
        (
            SELECT TOP (100)
                exercice.code AS exerciseCode,
                exercice.Caption AS exerciseCaption,
                exercice.dateDebut AS exerciseStart,
                exercice.dateFin AS exerciseEnd,
                hypothese.Caption AS hypothesisCaption,
                poste.code AS postCode,
                poste.Caption AS postCaption,
                periode.noPeriodeBudget AS budgetPeriodNumber,
                periode.Caption AS periodCaption,
                periode.dateDebut AS periodStart,
                periode.dateFin AS periodEnd,
                element_exercice.montant AS annualBudgetAmount,
                element_periode.montant AS periodBudgetAmount,
                element_periode.montant_CodeDevise AS currencyCode
            FROM dbo.TELEMENTBUDGETPERIODE element_periode
            INNER JOIN dbo.TELEMENTBUDGETEXERCICE element_exercice
                ON element_exercice.oid = element_periode.oidElementBudgetExercice
            LEFT JOIN dbo.THYPOTHESEBUDGETAIRE hypothese
                ON hypothese.oid = element_exercice.oidHypotheseBudgetaire
            LEFT JOIN dbo.TBUDGETEXERCICE budget_exercice
                ON budget_exercice.oid = hypothese.oidBudgetExercice
            LEFT JOIN dbo.TEXERCICEBUDGETAIRE exercice
                ON exercice.oid = budget_exercice.oidExerciceBudgetaire
            LEFT JOIN dbo.TPOSTE poste
                ON poste.oid = element_exercice.oidPosteBudgetaire
            LEFT JOIN dbo.TPERIODE periode
                ON periode.oid = element_periode.oidPeriode
            ORDER BY exercice.dateDebut DESC, hypothese.Caption, poste.code, periode.dateDebut
            FOR JSON PATH, INCLUDE_NULL_VALUES
        );
END;

SELECT SectionName, ObjectName, RowCount, Details
FROM #diagnostic
ORDER BY SortOrder;
