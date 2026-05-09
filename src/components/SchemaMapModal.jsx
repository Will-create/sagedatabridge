import { useEffect, useMemo, useState } from "react";
import { getRelationships } from "../hooks/useTauri";
import { useT } from "../i18n";

const NODE_WIDTH = 196;
const NODE_HEIGHT = 68;
const GRAPH_WIDTH = 920;

function tableKey(schema, table) {
  return `${schema}.${table}`;
}

function fullName(schema, table) {
  return `[${schema}].[${table}]`;
}

function compactName(schema, table) {
  return `${schema}.${table}`;
}

function distributeVertically(count, height, itemHeight) {
  if (count <= 0) return [];
  const available = Math.max(height - itemHeight, 0);
  if (count === 1) return [available / 2];
  const step = available / (count - 1);
  return Array.from({ length: count }, (_, index) => step * index);
}

function GraphNode({ node, x, y, variant, onClick }) {
  return (
    <g transform={`translate(${x}, ${y})`} onClick={() => onClick(node.key)} className={`schema-graph-node ${variant}`}>
      <rect rx="14" ry="14" width={NODE_WIDTH} height={NODE_HEIGHT} />
      <text x="14" y="24" className="schema-graph-node-title">{node.table}</text>
      <text x="14" y="42" className="schema-graph-node-subtitle">{node.schema}</text>
      <text x={NODE_WIDTH - 14} y="24" textAnchor="end" className="schema-graph-node-count">
        {node.metrics.total}
      </text>
      <text x={NODE_WIDTH - 14} y="44" textAnchor="end" className="schema-graph-node-count-detail">
        ← {node.metrics.inbound} / {node.metrics.outbound} →
      </text>
    </g>
  );
}

function GraphEdge({ from, to, direction }) {
  const startX = from.x + NODE_WIDTH;
  const startY = from.y + NODE_HEIGHT / 2;
  const endX = to.x;
  const endY = to.y + NODE_HEIGHT / 2;
  const dx = Math.abs(endX - startX) * 0.42;
  const path = `M ${startX} ${startY} C ${startX + dx} ${startY}, ${endX - dx} ${endY}, ${endX} ${endY}`;
  return <path d={path} className={`schema-graph-edge ${direction}`} markerEnd="url(#schemaArrow)" />;
}

function RelationshipCard({ title, groups, emptyLabel, onFocusTable, onBrowseTable, canBrowseTable, t }) {
  return (
    <section className="schema-detail-card">
      <div className="schema-detail-title">{title}</div>
      {groups.length === 0 ? (
        <div className="schema-detail-empty">{emptyLabel}</div>
      ) : (
        <div className="schema-detail-list">
          {groups.map((group) => (
            <div key={group.id} className="schema-detail-item">
              <div className="schema-detail-item-top">
                <button className="schema-table-link" onClick={() => onFocusTable(group.otherKey)}>
                  {compactName(group.otherSchema, group.otherTable)}
                </button>
                <div className="schema-detail-actions">
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() => onBrowseTable(group.otherKey)}
                    disabled={!canBrowseTable(group.otherKey)}
                  >
                    {t("schema_browse")}
                  </button>
                </div>
              </div>
              <div className="schema-detail-meta">{group.constraintName}</div>
              <div className="schema-detail-columns">
                {group.columns.map((column) => `${column.left} → ${column.right}`).join(", ")}
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

export default function SchemaMapModal({
  activeConn,
  activeTable,
  tables,
  onSelectTable,
  onClose,
  onToast,
}) {
  const { t } = useT();
  const [relationships, setRelationships] = useState([]);
  const [loading, setLoading] = useState(true);
  const [focusKey, setFocusKey] = useState(activeTable ? tableKey(activeTable.schema, activeTable.name) : "");
  const [search, setSearch] = useState("");

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      setLoading(true);
      try {
        const result = await getRelationships(activeConn);
        if (!cancelled) setRelationships(result);
      } catch (err) {
        if (!cancelled) {
          onToast({ type: "error", msg: String(err) });
          onClose();
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    load();
    return () => {
      cancelled = true;
    };
  }, [activeConn, onClose, onToast]);

  useEffect(() => {
    if (activeTable) {
      setFocusKey(tableKey(activeTable.schema, activeTable.name));
    }
  }, [activeTable]);

  const groupedRelationships = useMemo(() => {
    const grouped = new Map();
    relationships.forEach((relation) => {
      const sourceKey = tableKey(relation.source_schema, relation.source_table);
      const targetKey = tableKey(relation.target_schema, relation.target_table);
      const id = `${relation.constraint_name}:${sourceKey}:${targetKey}`;
      const current = grouped.get(id) || {
        id,
        constraintName: relation.constraint_name,
        sourceKey,
        targetKey,
        sourceSchema: relation.source_schema,
        sourceTable: relation.source_table,
        targetSchema: relation.target_schema,
        targetTable: relation.target_table,
        columns: [],
      };
      current.columns.push({
        ordinal: relation.ordinal,
        source: relation.source_column,
        target: relation.target_column,
      });
      grouped.set(id, current);
    });

    return [...grouped.values()]
      .map((group) => ({
        ...group,
        columns: [...group.columns].sort((a, b) => a.ordinal - b.ordinal),
      }))
      .sort((a, b) =>
        a.sourceSchema.localeCompare(b.sourceSchema)
        || a.sourceTable.localeCompare(b.sourceTable)
        || a.constraintName.localeCompare(b.constraintName),
      );
  }, [relationships]);

  const tableMetrics = useMemo(() => {
    const metrics = new Map();
    const ensure = (key, schema, table) => {
      if (!metrics.has(key)) {
        metrics.set(key, { key, schema, table, inbound: 0, outbound: 0, total: 0 });
      }
      return metrics.get(key);
    };

    groupedRelationships.forEach((group) => {
      const source = ensure(group.sourceKey, group.sourceSchema, group.sourceTable);
      const target = ensure(group.targetKey, group.targetSchema, group.targetTable);
      source.outbound += 1;
      source.total += 1;
      target.inbound += 1;
      target.total += 1;
    });

    return metrics;
  }, [groupedRelationships]);

  const allTables = useMemo(() => {
    const map = new Map();
    (tables || []).forEach((table) => {
      const key = tableKey(table.schema, table.name);
      map.set(key, {
        key,
        schema: table.schema,
        table: table.name,
        fullName: table.full_name || fullName(table.schema, table.name),
      });
    });

    groupedRelationships.forEach((group) => {
      if (!map.has(group.sourceKey)) {
        map.set(group.sourceKey, {
          key: group.sourceKey,
          schema: group.sourceSchema,
          table: group.sourceTable,
          fullName: fullName(group.sourceSchema, group.sourceTable),
        });
      }
      if (!map.has(group.targetKey)) {
        map.set(group.targetKey, {
          key: group.targetKey,
          schema: group.targetSchema,
          table: group.targetTable,
          fullName: fullName(group.targetSchema, group.targetTable),
        });
      }
    });

    return [...map.values()].sort((a, b) =>
      a.schema.localeCompare(b.schema) || a.table.localeCompare(b.table),
    );
  }, [groupedRelationships, tables]);

  const tableByKey = useMemo(() => {
    const map = new Map();
    allTables.forEach((table) => map.set(table.key, table));
    return map;
  }, [allTables]);

  useEffect(() => {
    if (!focusKey && allTables.length > 0) {
      setFocusKey(allTables[0].key);
      return;
    }
    if (focusKey && !tableByKey.has(focusKey) && allTables.length > 0) {
      setFocusKey(allTables[0].key);
    }
  }, [allTables, focusKey, tableByKey]);

  const focusTable = focusKey ? tableByKey.get(focusKey) : null;

  const outgoingGroups = useMemo(
    () => groupedRelationships
      .filter((group) => group.sourceKey === focusKey && group.targetKey !== focusKey)
      .map((group) => ({
        ...group,
        otherKey: group.targetKey,
        otherSchema: group.targetSchema,
        otherTable: group.targetTable,
      })),
    [focusKey, groupedRelationships],
  );

  const incomingGroups = useMemo(
    () => groupedRelationships
      .filter((group) => group.targetKey === focusKey && group.sourceKey !== focusKey)
      .map((group) => ({
        ...group,
        otherKey: group.sourceKey,
        otherSchema: group.sourceSchema,
        otherTable: group.sourceTable,
        columns: group.columns.map((column) => ({
          ...column,
          left: column.source,
          right: column.target,
        })),
      }))
      .map((group) => ({
        ...group,
        columns: group.columns,
      })),
    [focusKey, groupedRelationships],
  );

  const outgoingCards = useMemo(
    () => outgoingGroups.map((group) => ({
      ...group,
      columns: group.columns.map((column) => ({
        left: column.source,
        right: column.target,
      })),
    })),
    [outgoingGroups],
  );

  const selfGroups = useMemo(
    () => groupedRelationships.filter((group) => group.sourceKey === focusKey && group.targetKey === focusKey),
    [focusKey, groupedRelationships],
  );

  const relatedTableCount = useMemo(() => {
    const related = new Set();
    outgoingGroups.forEach((group) => related.add(group.otherKey));
    incomingGroups.forEach((group) => related.add(group.otherKey));
    return related.size;
  }, [incomingGroups, outgoingGroups]);

  const filteredTables = useMemo(() => {
    const query = search.trim().toLowerCase();
    return allTables.filter((table) => {
      if (!query) return true;
      return table.schema.toLowerCase().includes(query) || table.table.toLowerCase().includes(query);
    });
  }, [allTables, search]);

  const browserTableKeys = useMemo(
    () => new Set((tables || []).map((table) => tableKey(table.schema, table.name))),
    [tables],
  );

  const graphModel = useMemo(() => {
    if (!focusTable) return null;

    const focusMetrics = tableMetrics.get(focusTable.key) || {
      inbound: 0,
      outbound: 0,
      total: 0,
    };

    const uniqueNodes = (groups) => {
      const nodes = new Map();
      groups.forEach((group) => {
        if (nodes.has(group.otherKey)) return;
        const table = tableByKey.get(group.otherKey);
        const metrics = tableMetrics.get(group.otherKey) || { inbound: 0, outbound: 0, total: 0 };
        nodes.set(group.otherKey, {
          key: group.otherKey,
          schema: table?.schema || group.otherSchema,
          table: table?.table || group.otherTable,
          metrics,
        });
      });
      return [...nodes.values()];
    };

    const incomingNodes = uniqueNodes(incomingGroups);
    const outgoingNodes = uniqueNodes(outgoingGroups);

    const maxRows = Math.max(incomingNodes.length, outgoingNodes.length, 1);
    const height = Math.max(360, maxRows * 94 + 116);
    const centerX = (GRAPH_WIDTH - NODE_WIDTH) / 2;
    const centerY = (height - NODE_HEIGHT) / 2;
    const incomingY = distributeVertically(incomingNodes.length, height, NODE_HEIGHT);
    const outgoingY = distributeVertically(outgoingNodes.length, height, NODE_HEIGHT);

    return {
      height,
      focus: {
        key: focusTable.key,
        schema: focusTable.schema,
        table: focusTable.table,
        metrics: focusMetrics,
        x: centerX,
        y: centerY,
      },
      incoming: incomingNodes.map((node, index) => ({ ...node, x: 42, y: incomingY[index] })),
      outgoing: outgoingNodes.map((node, index) => ({
        ...node,
        x: GRAPH_WIDTH - NODE_WIDTH - 42,
        y: outgoingY[index],
      })),
    };
  }, [focusTable, incomingGroups, outgoingGroups, tableByKey, tableMetrics]);

  const handleBrowseTable = (key) => {
    const table = (tables || []).find((item) => tableKey(item.schema, item.name) === key);
    if (!table) return;
    onSelectTable(table);
    onClose();
  };

  return (
    <div className="modal-overlay" onClick={(event) => event.target === event.currentTarget && onClose()}>
      <div className="modal schema-map-modal">
        <div className="modal-title">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="3" y="5" width="7" height="5" rx="1" />
            <rect x="14" y="3" width="7" height="5" rx="1" />
            <rect x="14" y="16" width="7" height="5" rx="1" />
            <path d="M10 7h4M12 7v11M12 18h2" />
          </svg>
          {t("schema_title")}
        </div>

        <div className="modal-body modal-body-fill">
          {loading ? (
            <div className="schema-loading">
              <div className="spinner" />
              <div>{t("schema_loading")}</div>
            </div>
          ) : (
            <div className="schema-map-layout">
              <aside className="schema-map-sidebar">
                <div className="schema-search-card">
                  <div className="schema-sidebar-title">{t("schema_tables")}</div>
                  <input
                    type="text"
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder={t("schema_search_ph")}
                  />
                </div>

                <div className="schema-table-list">
                  {filteredTables.map((table) => {
                    const metrics = tableMetrics.get(table.key) || { inbound: 0, outbound: 0, total: 0 };
                    const active = table.key === focusKey;
                    return (
                      <button
                        key={table.key}
                        className={`schema-table-item ${active ? "active" : ""}`}
                        onClick={() => setFocusKey(table.key)}
                      >
                        <strong>{table.table}</strong>
                        <span>{table.schema}</span>
                        <small>← {metrics.inbound} / {metrics.outbound} →</small>
                      </button>
                    );
                  })}
                </div>
              </aside>

              <div className="schema-map-main">
                <div className="schema-summary-grid">
                  <div className="schema-stat-card">
                    <span>{t("schema_focus")}</span>
                    <strong>{focusTable ? compactName(focusTable.schema, focusTable.table) : t("schema_none")}</strong>
                  </div>
                  <div className="schema-stat-card">
                    <span>{t("schema_relationships")}</span>
                    <strong>{relationships.length}</strong>
                  </div>
                  <div className="schema-stat-card">
                    <span>{t("schema_related_tables")}</span>
                    <strong>{relatedTableCount}</strong>
                  </div>
                </div>

                <div className="schema-graph-card">
                  <div className="schema-detail-title">{t("schema_graph")}</div>
                  {!graphModel || (graphModel.incoming.length === 0 && graphModel.outgoing.length === 0 && selfGroups.length === 0) ? (
                    <div className="schema-detail-empty">{t("schema_no_relations")}</div>
                  ) : (
                    <svg
                      viewBox={`0 0 ${GRAPH_WIDTH} ${graphModel.height}`}
                      className="schema-graph"
                      role="img"
                      aria-label={t("schema_graph")}
                    >
                      <defs>
                        <marker id="schemaArrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                          <path d="M 0 0 L 10 5 L 0 10 z" className="schema-graph-arrow" />
                        </marker>
                      </defs>

                      {graphModel.incoming.map((node) => (
                        <GraphEdge
                          key={`incoming-${node.key}`}
                          from={node}
                          to={graphModel.focus}
                          direction="in"
                        />
                      ))}
                      {graphModel.outgoing.map((node) => (
                        <GraphEdge
                          key={`outgoing-${node.key}`}
                          from={graphModel.focus}
                          to={node}
                          direction="out"
                        />
                      ))}

                      {graphModel.incoming.map((node) => (
                        <GraphNode key={node.key} node={node} x={node.x} y={node.y} variant="secondary" onClick={setFocusKey} />
                      ))}
                      <GraphNode
                        node={graphModel.focus}
                        x={graphModel.focus.x}
                        y={graphModel.focus.y}
                        variant="focus"
                        onClick={setFocusKey}
                      />
                      {graphModel.outgoing.map((node) => (
                        <GraphNode key={node.key} node={node} x={node.x} y={node.y} variant="secondary" onClick={setFocusKey} />
                      ))}
                    </svg>
                  )}
                </div>

                {selfGroups.length > 0 && (
                  <div className="schema-self-note">
                    {t("schema_self_refs", selfGroups.length)}
                  </div>
                )}

                <div className="schema-detail-grid">
                  <RelationshipCard
                    title={t("schema_references")}
                    groups={outgoingCards}
                    emptyLabel={t("schema_references_empty")}
                    onFocusTable={setFocusKey}
                    onBrowseTable={handleBrowseTable}
                    canBrowseTable={(key) => browserTableKeys.has(key)}
                    t={t}
                  />
                  <RelationshipCard
                    title={t("schema_referenced_by")}
                    groups={incomingGroups}
                    emptyLabel={t("schema_referenced_by_empty")}
                    onFocusTable={setFocusKey}
                    onBrowseTable={handleBrowseTable}
                    canBrowseTable={(key) => browserTableKeys.has(key)}
                    t={t}
                  />
                </div>
              </div>
            </div>
          )}
        </div>

        <div className="modal-actions">
          {focusTable && (
            <button
              className="btn"
              onClick={() => handleBrowseTable(focusTable.key)}
              disabled={!browserTableKeys.has(focusTable.key)}
            >
              {t("schema_browse_focus")}
            </button>
          )}
          <div style={{ flex: 1 }} />
          <button className="btn btn-ghost" onClick={onClose}>{t("close")}</button>
        </div>
      </div>
    </div>
  );
}
