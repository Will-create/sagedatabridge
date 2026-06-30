SELECT
    s.name AS SchemaName,
    t.name AS TableName,
    CAST(tblDesc.value AS NVARCHAR(MAX)) AS TableDescription,
    c.column_id AS ColumnOrder,
    c.name AS ColumnName,
    ty.name AS DataType,
    c.max_length AS MaxLength,
    c.precision,
    c.scale,
    c.is_nullable AS IsNullable,
    CAST(colDesc.value AS NVARCHAR(MAX)) AS ColumnDescription
FROM sys.tables t
INNER JOIN sys.schemas s
    ON t.schema_id = s.schema_id
INNER JOIN sys.columns c
    ON t.object_id = c.object_id
INNER JOIN sys.types ty
    ON c.user_type_id = ty.user_type_id
LEFT JOIN sys.extended_properties tblDesc
    ON tblDesc.major_id = t.object_id
    AND tblDesc.minor_id = 0
    AND tblDesc.name = 'MS_Description'
LEFT JOIN sys.extended_properties colDesc
    ON colDesc.major_id = c.object_id
    AND colDesc.minor_id = c.column_id
    AND colDesc.name = 'MS_Description'
WHERE t.is_ms_shipped = 0
ORDER BY
    s.name,
    t.name,
    c.column_id;