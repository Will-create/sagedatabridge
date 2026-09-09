import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/api/dialog";
import { listSqlServerPaths } from "../../hooks/useTauri";
import { useT } from "../../i18n";
import {
  isUserProfilePath,
  likelyStripeMatch,
  parentBackupDirectory,
  repairedDatabaseName,
  salvageTargetAllowed,
  stripeNameHints,
} from "../operationsModel";

function formatBytes(bytes) {
  const value = Number(bytes || 0);
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)} GB`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)} MB`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(0)} KB`;
  return `${value} B`;
}

export default function SalvageRescuePanel({
  restore,
  setRestore,
  inspection,
  inspecting,
  stagePlan,
  staging,
  onPickLocal,
  onPreviewStage,
  onStagePath,
  onConfirmStage,
  onAddStripe,
  onRemoveStripe,
  onBrowseSql,
  onBrowseStripe,
}) {
  const { t } = useT();
  const [siblings, setSiblings] = useState([]);
  const classification = inspection?.classification || (inspecting ? "scanning" : "idle");
  const tone = useMemo(() => {
    if (inspecting || classification === "scanning") return "scanning";
    if (classification === "healthy") return "healthy";
    if (classification === "checksum_damage") return "salvage";
    if (classification === "missing_media_family") return "stripe";
    if (classification === "access_denied") return "denied";
    if (classification === "incomplete" || classification === "header_destroyed") return "lost";
    return "idle";
  }, [classification, inspecting]);

  const pickBak = async (handler) => {
    const path = await open({
      multiple: false,
      filters: [{ name: "SQL Server backup", extensions: ["bak"] }],
    });
    if (path) handler?.(path);
  };

  const familyCount = Number(inspection?.familyCount || 0);
  const provided = inspection?.providedFamilies || [];
  const missing = inspection?.missingFamilies || [];
  const showGuide = Boolean(inspection) && classification !== "scanning" && classification !== "idle";
  const hints = stripeNameHints(restore.backupPath);
  const nextMissing = missing[0] || (familyCount > 1 ? familyCount : 1);
  const attachedFiles = [restore.backupPath, ...(restore.extraBackupPaths || [])].filter(Boolean);

  useEffect(() => {
    if (!restore.connectionId || !restore.backupPath || familyCount < 2) {
      setSiblings([]);
      return;
    }
    const directory = parentBackupDirectory(restore.backupPath);
    if (!directory || isUserProfilePath(directory)) {
      setSiblings([]);
      return;
    }
    let cancelled = false;
    listSqlServerPaths(restore.connectionId, directory)
      .then((listing) => {
        if (cancelled) return;
        setSiblings((listing.entries || []).filter((entry) => (
          entry.isBackup
          && likelyStripeMatch(entry.name, restore.backupPath, hints)
          && !attachedFiles.some((path) => path.toLowerCase() === String(entry.path || "").toLowerCase())
        )));
      })
      .catch(() => {
        if (!cancelled) setSiblings([]);
      });
    return () => { cancelled = true; };
  }, [familyCount, restore.backupPath, restore.connectionId, restore.extraBackupPaths]);

  return (
    <div className={`salvage-rescue operations-span-2 tone-${tone}`}>
      <div className="salvage-rescue-stage" aria-hidden="true">
        <span className="salvage-orbit salvage-orbit-a" />
        <span className="salvage-orbit salvage-orbit-b" />
        <span className="salvage-scan" />
        <div className="salvage-core">
          <strong>.bak</strong>
          <small>{inspection?.databaseName || t("operations_salvage_waiting")}</small>
        </div>
      </div>

      <div className="salvage-rescue-copy">
        <p className="salvage-kicker">{t("operations_salvage_kicker")}</p>
        <h3>{t("operations_salvage_title")}</h3>
        <p>{t(`operations_class_${classification}`)}</p>

        {showGuide ? (
          <div className="salvage-guide">
            {inspection?.databaseName || inspection?.serverName || familyCount > 1 ? (
              <ul className="salvage-facts">
                {inspection?.databaseName ? <li>{t("operations_fact_database", inspection.databaseName)}</li> : null}
                {inspection?.serverName ? <li>{t("operations_fact_server", inspection.serverName)}</li> : null}
                {inspection?.backupFinishDate ? <li>{t("operations_fact_when", inspection.backupFinishDate)}</li> : null}
                {inspection?.backupSize ? <li>{t("operations_fact_size", inspection.backupSize)}</li> : null}
                {familyCount > 1 ? <li>{t("operations_fact_this_stripe", inspection.familySequence || 1, familyCount)}</li> : null}
                {inspection?.mediaSetId ? <li>{t("operations_fact_media_set", inspection.mediaSetId)}</li> : null}
              </ul>
            ) : null}
            <section>
              <strong>{t("operations_guide_explain")}</strong>
              <p>{t(`operations_explain_${classification}`)}</p>
            </section>
            <section>
              <strong>{t("operations_guide_recommend")}</strong>
              <ol>
                <li>{t(`operations_recommend_${classification}_1`)}</li>
                <li>{t(`operations_recommend_${classification}_2`)}</li>
                <li>{t(`operations_recommend_${classification}_3`)}</li>
              </ol>
            </section>
            <section>
              <strong>{t("operations_guide_actions")}</strong>
              <div className="salvage-guide-actions">
                {classification === "missing_media_family" ? (
                  <>
                    <button type="button" className="btn btn-accent btn-sm" onClick={() => pickBak(onAddStripe)}>
                      {t("operations_stripe_add_pc", nextMissing)}
                    </button>
                    <button type="button" className="btn btn-ghost btn-sm" disabled={!restore.connectionId} onClick={onBrowseStripe}>
                      {t("operations_stripe_add_sql", nextMissing)}
                    </button>
                  </>
                ) : null}
                {classification === "access_denied" ? (
                  <>
                    <button type="button" className="btn btn-accent btn-sm" disabled={staging || !restore.connectionId} onClick={onPreviewStage}>
                      {t("operations_stage_preview")}
                    </button>
                    <button type="button" className="btn btn-ghost btn-sm" disabled={!restore.connectionId} onClick={onBrowseSql}>
                      {t("operations_browse_sql_bak")}
                    </button>
                  </>
                ) : null}
                {classification === "incomplete" || classification === "header_destroyed" ? (
                  <>
                    <button type="button" className="btn btn-accent btn-sm" onClick={() => pickBak(onPickLocal)}>
                      {t("operations_pick_another_bak")}
                    </button>
                    <button type="button" className="btn btn-ghost btn-sm" disabled={!restore.connectionId} onClick={onBrowseSql}>
                      {t("operations_browse_sql_bak")}
                    </button>
                  </>
                ) : null}
                {classification === "checksum_damage" ? (
                  <button
                    type="button"
                    className="btn btn-accent btn-sm"
                    onClick={() => setRestore((current) => ({
                      ...current,
                      salvage: true,
                      targetDatabase: current.targetDatabase && salvageTargetAllowed(current.targetDatabase)
                        ? current.targetDatabase
                        : inspection.suggestedTarget || repairedDatabaseName(inspection.databaseName || "database"),
                      confirmationText: "",
                    }))}
                  >
                    {t("operations_enable_salvage_action")}
                  </button>
                ) : null}
                {classification === "healthy" ? (
                  <p>{t("operations_recommend_healthy_2")}</p>
                ) : null}
              </div>
            </section>
          </div>
        ) : null}

        {familyCount > 1 ? (
          <div className="salvage-stripes">
            <strong>{t("operations_stripe_title", familyCount)}</strong>
            <div className="salvage-stripe-slots">
              {Array.from({ length: familyCount }, (_, index) => {
                const sequence = index + 1;
                const filled = provided.includes(sequence);
                return (
                  <span key={sequence} className={filled ? "lit" : "warn"}>
                    {filled ? t("operations_stripe_have", sequence) : t("operations_stripe_need", sequence)}
                  </span>
                );
              })}
            </div>
            {hints.length ? <p>{t("operations_stripe_hints", hints.slice(0, 3).join(", "))}</p> : null}
            {missing.length ? (
              <div className="salvage-guide-actions">
                <button type="button" className="btn btn-accent btn-sm" onClick={() => pickBak(onAddStripe)}>
                  {t("operations_stripe_add", missing[0])}
                </button>
                <button type="button" className="btn btn-ghost btn-sm" disabled={!restore.connectionId} onClick={onBrowseStripe}>
                  {t("operations_stripe_add_sql", missing[0])}
                </button>
              </div>
            ) : (
              <div className="operations-warning">{t("operations_stripe_complete")}</div>
            )}
            {siblings.length ? (
              <div className="salvage-matches">
                <strong>{t("operations_stripe_matches")}</strong>
                {siblings.map((entry) => (
                  <button key={entry.path} type="button" className="btn btn-ghost btn-sm" onClick={() => onAddStripe?.(entry.path)}>
                    {t("operations_stripe_attach")}: {entry.name}
                  </button>
                ))}
              </div>
            ) : null}
            <ul className="salvage-stripe-files">
              {attachedFiles.map((path, index) => (
                <li key={path}>
                  <code>{path}</code>
                  {index > 0 ? (
                    <button type="button" className="btn btn-ghost btn-sm" onClick={() => onRemoveStripe?.(path)}>
                      {t("operations_stripe_remove")}
                    </button>
                  ) : null}
                  {isUserProfilePath(path) ? (
                    <button type="button" className="btn btn-ghost btn-sm" disabled={staging || !restore.connectionId} onClick={() => onStagePath?.(path)}>
                      {t("operations_stage_this_file")}
                    </button>
                  ) : null}
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        <div className="salvage-orbs" role="list">
          <span className={inspection?.headerOk ? "lit" : inspecting ? "pulse" : ""}>{t("operations_salvage_header")}</span>
          <span className={inspection?.fileListOk ? "lit" : inspecting ? "pulse" : ""}>{t("operations_salvage_files")}</span>
          <span className={inspection?.verifyOk ? "lit" : inspecting ? "pulse" : ""}>{t("operations_salvage_verify")}</span>
          {inspection?.familyCount > 1 ? (
            <span className={missing.length ? "warn" : "lit"}>{t("operations_salvage_stripes", inspection.familySequence || 1, inspection.familyCount)}</span>
          ) : null}
        </div>

        <div className="salvage-actions">
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => pickBak(onPickLocal)}>{t("operations_pick_local_bak")}</button>
          {restore.backupPath && isUserProfilePath(restore.backupPath) ? (
            <button type="button" className="btn btn-ghost btn-sm" disabled={staging || !restore.connectionId} onClick={onPreviewStage}>
              {t("operations_stage_preview")}
            </button>
          ) : null}
        </div>

        {stagePlan ? (
          <div className="salvage-stage-card">
            <strong>{t("operations_stage_title")}</strong>
            <p>{t("operations_stage_body", formatBytes(stagePlan.bytes), stagePlan.sqlAccount || "SQL Server")}</p>
            <code>{stagePlan.sourcePath}</code>
            <span className="salvage-arrow">↓</span>
            <code>{stagePlan.destinationPath}</code>
            {stagePlan.warnings?.map((warning) => <div className="operations-warning" key={warning}>{warning}</div>)}
            {stagePlan.needsStage ? (
              <button type="button" className="btn btn-accent" disabled={staging} onClick={onConfirmStage}>
                {staging ? t("operations_working") : t("operations_stage_confirm")}
              </button>
            ) : (
              <div className="operations-warning">{t("operations_stage_done")}</div>
            )}
          </div>
        ) : null}

        {inspection?.messages?.map((message) => (
          <div className={inspection.unrecoverable ? "operations-error" : "operations-warning"} key={message}>{message}</div>
        ))}

        {inspection?.salvageable ? (
          <>
            <label className="operations-checkbox">
              <input
                type="checkbox"
                checked={restore.salvage}
                onChange={(event) => setRestore((current) => ({
                  ...current,
                  salvage: event.target.checked,
                  targetDatabase: event.target.checked
                    ? (current.targetDatabase && salvageTargetAllowed(current.targetDatabase) ? current.targetDatabase : inspection.suggestedTarget || repairedDatabaseName("database"))
                    : current.targetDatabase,
                  confirmationText: "",
                }))}
              />
              {t("operations_salvage_enable", inspection.suggestedTarget || "database_repaired")}
            </label>
            {restore.salvage ? (
              <>
                <label>
                  {t("operations_salvage_repair")}
                  <select value={restore.repairLevel} onChange={(event) => setRestore((current) => ({ ...current, repairLevel: event.target.value, allowDataLossConfirmation: "" }))}>
                    <option value="none">{t("operations_salvage_repair_none")}</option>
                    <option value="rebuild">{t("operations_salvage_repair_rebuild")}</option>
                    <option value="allow_data_loss">{t("operations_salvage_repair_loss")}</option>
                  </select>
                </label>
                {restore.repairLevel === "allow_data_loss" ? (
                  <label>
                    {t("operations_salvage_loss_confirm")}
                    <input value={restore.allowDataLossConfirmation} onChange={(event) => setRestore((current) => ({ ...current, allowDataLossConfirmation: event.target.value }))} />
                  </label>
                ) : null}
              </>
            ) : null}
          </>
        ) : null}
      </div>
    </div>
  );
}
