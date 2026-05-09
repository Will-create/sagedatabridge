import { useT } from "../i18n";

export default function StatusBar({ message, type, elapsed }) {
  const { t } = useT();
  const cls = type === "error" ? "error" : type === "success" ? "" : "idle";
  return (
    <div className={`status-bar ${cls}`}>
      <div className="status-dot" />
      <span>{message || t("ready")}</span>
      {elapsed != null && (
        <span style={{ marginLeft:"auto", opacity:0.7, fontSize:10 }}>{elapsed}ms</span>
      )}
    </div>
  );
}
