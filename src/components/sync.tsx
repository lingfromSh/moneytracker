import * as React from "react";
import { CloudUpload, RefreshCw, ShieldCheck } from "lucide-react";
import type {
  SyncChoiceDto,
  SyncConfigDto_Serialize,
  SyncConflictDto_Deserialize,
} from "../lib/bindings";
import { api } from "../lib/ipc";
import { Badge, Button, Card, CardHeader, Field, Input, Modal, Switch } from "./ui";

type Props = {
  onMessage: (message: string | null) => void;
  onError: (message: string | null) => void;
};

type Form = {
  enabled: boolean;
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  pathStyle: boolean;
  auto: boolean;
  device: string;
};

function formOf(config: SyncConfigDto_Serialize): Form {
  return {
    enabled: config.enabled,
    endpoint: config.endpoint,
    region: config.region,
    bucket: config.bucket,
    prefix: config.prefix,
    pathStyle: config.path_style,
    auto: config.auto,
    device: config.device,
  };
}

function when(iso: string | null | undefined): string {
  if (!iso) return "从未";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString("zh-CN", { hour12: false });
}

/**
 * Multi-device sync over an S3-compatible bucket (MinIO, R2, S3 …). The vault
 * stays plain text; this only mirrors `.ledger` files and keeps a manifest so a
 * file edited on two devices is *stopped on*, not silently overwritten.
 */
export function SyncSettings({ onMessage, onError }: Props) {
  const [config, setConfig] = React.useState<SyncConfigDto_Serialize | null>(null);
  const [form, setForm] = React.useState<Form | null>(null);
  const [accessKey, setAccessKey] = React.useState("");
  const [secretKey, setSecretKey] = React.useState("");
  const [busy, setBusy] = React.useState<string | null>(null);
  const [conflicts, setConflicts] = React.useState<SyncConflictDto_Deserialize[]>([]);
  const [choices, setChoices] = React.useState<Record<string, "local" | "remote">>({});

  const load = React.useCallback(async () => {
    try {
      const next = await api.syncConfig();
      setConfig(next);
      setForm(formOf(next));
    } catch (cause) {
      onError((cause as Error).message);
    }
  }, [onError]);

  React.useEffect(() => {
    void load();
  }, [load]);

  const patch = (partial: Partial<Form>) => {
    setForm((current) => (current ? { ...current, ...partial } : current));
    onMessage(null);
  };

  const run = async (label: string, work: () => Promise<string | null>) => {
    setBusy(label);
    onError(null);
    onMessage(null);
    try {
      const message = await work();
      if (message) onMessage(message);
    } catch (cause) {
      onError((cause as Error).message);
    } finally {
      setBusy(null);
    }
  };

  const saveForm = () =>
    run("save", async () => {
      if (!form) return null;
      const saved = await api.syncSave({
        enabled: form.enabled,
        endpoint: form.endpoint,
        region: form.region,
        bucket: form.bucket,
        prefix: form.prefix,
        path_style: form.pathStyle,
        auto: form.auto,
        device: form.device,
      });
      setConfig(saved);
      setForm(formOf(saved));
      return "同步设置已保存";
    });

  const saveKeys = () =>
    run("keys", async () => {
      const saved = await api.syncSetKeys(accessKey, secretKey);
      setConfig(saved);
      setAccessKey("");
      setSecretKey("");
      return accessKey.trim() ? "密钥已保存到本机" : "密钥已清除";
    });

  const test = () =>
    run("test", async () => {
      if (form) await api.syncSave({ ...form, path_style: form.pathStyle });
      return await api.syncTest();
    });

  const preview = () =>
    run("preview", async () => {
      const result = await api.syncPreview();
      const uploads = result.uploads?.length ?? 0;
      const downloads = result.downloads?.length ?? 0;
      const remoteDeletes = result.delete_remote?.length ?? 0;
      const localDeletes = result.delete_local?.length ?? 0;
      const clash = result.conflicts?.length ?? 0;
      if (uploads + downloads + remoteDeletes + localDeletes + clash === 0) {
        return "两边一致，没有要同步的内容";
      }
      const parts = [`上传 ${uploads}`, `下载 ${downloads}`];
      if (remoteDeletes) parts.push(`云端删除 ${remoteDeletes}`);
      if (localDeletes) parts.push(`本机删除 ${localDeletes}`);
      if (clash) parts.push(`冲突 ${clash}`);
      return parts.join(" · ");
    });

  const sync = () =>
    run("sync", async () => {
      if (form) await api.syncSave({ ...form, path_style: form.pathStyle });
      const report = await api.syncRun([]);
      if (!report.complete) {
        const list = report.conflicts ?? [];
        setConflicts(list);
        setChoices(Object.fromEntries(list.map((item) => [item.rel, "remote" as const])));
        return "有文件两边都改过，请选择保留哪一边";
      }
      setConflicts([]);
      await load();
      return report.summary ?? "同步完成";
    });

  const resolve = () =>
    run("resolve", async () => {
      const picked: SyncChoiceDto[] = conflicts.map((item) => ({
        rel: item.rel,
        side: choices[item.rel] ?? "remote",
      }));
      const report = await api.syncRun(picked);
      if (!report.complete) {
        return "还有文件存在冲突，请再检查一次";
      }
      setConflicts([]);
      await load();
      return report.summary ?? "同步完成";
    });

  if (!config || !form) {
    return (
      <Card>
        <CardHeader title="同步" hint="把账本镜像到自己的对象存储，多台设备共用一份" />
        <div className="p-4 text-xs text-[var(--color-muted)]">正在读取同步设置…</div>
      </Card>
    );
  }

  const configured = config.configured;
  const status = !form.enabled
    ? { tone: "neutral" as const, text: "未开启" }
    : conflicts.length > 0
      ? { tone: "warn" as const, text: "有冲突待处理" }
      : configured
        ? { tone: "accent" as const, text: `上次同步 ${when(config.last_sync)}` }
        : { tone: "warn" as const, text: "还没配置完" };

  return (
    <Card>
      <CardHeader
        title="同步"
        hint="账本文件镜像到 S3 / MinIO；两边都改过就停下来问你，不自动覆盖"
        action={<Badge tone={status.tone}>{status.text}</Badge>}
      />
      <div className="space-y-3 p-4">
        <div className="flex items-center justify-between rounded-lg border border-[var(--color-line)] px-3 py-2">
          <div>
            <div className="text-sm">启用同步</div>
            <div className="text-[11px] text-[var(--color-muted)]">
              {config.has_ledger ? "只在打开账本时生效" : "先打开一个账本"}
            </div>
          </div>
          <Switch checked={form.enabled} onChange={(next) => patch({ enabled: next })} label="启用同步" />
        </div>

        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="服务端点" hint="MinIO 例如 https://api.minio.example.com">
            <Input
              value={form.endpoint}
              placeholder="https://s3.example.com"
              onChange={(e) => patch({ endpoint: e.target.value.trim() })}
            />
          </Field>
          <Field label="Bucket">
            <Input
              value={form.bucket}
              placeholder="ledger"
              onChange={(e) => patch({ bucket: e.target.value.trim() })}
            />
          </Field>
          <Field label="前缀" hint="同一个 bucket 里区分多个账本">
            <Input
              value={form.prefix}
              placeholder="moneytracker"
              onChange={(e) => patch({ prefix: e.target.value.trim() })}
            />
          </Field>
          <Field label="区域" hint="MinIO 随便填，AWS 要填真实区域">
            <Input value={form.region} onChange={(e) => patch({ region: e.target.value.trim() })} />
          </Field>
          <Field label="设备名" hint="写进 manifest，方便看是谁最后同步的">
            <Input
              value={form.device}
              placeholder="Mac / 手机"
              onChange={(e) => patch({ device: e.target.value })}
            />
          </Field>
          <div className="flex items-end justify-between gap-2 rounded-lg border border-[var(--color-line)] px-3 py-2">
            <div>
              <div className="text-sm">路径风格访问</div>
              <div className="text-[11px] text-[var(--color-muted)]">MinIO 一般要打开</div>
            </div>
            <Switch
              checked={form.pathStyle}
              onChange={(next) => patch({ pathStyle: next })}
              label="路径风格访问"
            />
          </div>
        </div>

        <div className="grid gap-3 sm:grid-cols-2">
          <Field
            label="Access Key"
            hint={config.has_keys ? `已保存 ${config.access_key_hint}` : "还没保存"}
          >
            <Input
              value={accessKey}
              autoComplete="off"
              placeholder={config.has_keys ? "留空表示不修改" : "Access Key"}
              onChange={(e) => setAccessKey(e.target.value)}
            />
          </Field>
          <Field label="Secret Key" hint="和 LLM 密钥一样，存在本机 0600 文件里">
            <Input
              type="password"
              value={secretKey}
              autoComplete="off"
              placeholder={config.has_keys ? "留空表示不修改" : "Secret Key"}
              onChange={(e) => setSecretKey(e.target.value)}
            />
          </Field>
        </div>

        <div className="flex items-center justify-between rounded-lg border border-[var(--color-line)] px-3 py-2">
          <div>
            <div className="text-sm">启动时自动拉取</div>
            <div className="text-[11px] text-[var(--color-muted)]">
              打开后，启动时会先合并云端的改动，再进入账本
            </div>
          </div>
          <Switch checked={form.auto} onChange={(next) => patch({ auto: next })} label="启动时自动拉取" />
        </div>

        <div className="flex flex-wrap gap-2">
          <Button variant="primary" onClick={() => void sync()} disabled={!form.enabled || busy !== null}>
            <CloudUpload size={14} /> {busy === "sync" ? "同步中…" : "立即同步"}
          </Button>
          <Button variant="subtle" onClick={() => void preview()} disabled={!form.enabled || busy !== null}>
            <RefreshCw size={14} /> {busy === "preview" ? "检查中…" : "预览差异"}
          </Button>
          <Button variant="subtle" onClick={() => void test()} disabled={busy !== null}>
            <ShieldCheck size={14} /> {busy === "test" ? "连接中…" : "测试连接"}
          </Button>
          <Button variant="subtle" onClick={() => void saveForm()} disabled={busy !== null}>
            保存设置
          </Button>
          <Button
            variant="subtle"
            onClick={() => void saveKeys()}
            disabled={busy !== null || (!accessKey.trim() && !secretKey.trim())}
          >
            保存密钥
          </Button>
        </div>

        {!configured ? (
          <div className="text-[11px] text-[var(--color-muted)]">
            填好端点、bucket 和两个密钥后先点「测试连接」，确认能读到 bucket 再点「立即同步」。
          </div>
        ) : null}
      </div>

      <Modal open={conflicts.length > 0} onClose={() => setConflicts([])} title="选择保留哪一边" width="max-w-2xl">
        <div className="space-y-3 p-4">
          <div className="text-xs text-[var(--color-muted)]">
            下面的文件在两边都被改过。选择「用云端」会用 bucket 里的版本覆盖本机（本机文件仍可从
            <code className="mx-1 rounded bg-[var(--color-surface-2)] px-1">.bak/</code>
            找回）；选择「保留本机」则把本机版本推上去。
          </div>
          <div className="max-h-72 space-y-2 overflow-y-auto">
            {conflicts.map((item) => (
              <div
                key={item.rel}
                className="rounded-lg border border-[var(--color-line)] px-3 py-2 text-xs"
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="font-mono">{item.rel}</span>
                  <div className="flex gap-1">
                    <Button
                      variant={choices[item.rel] === "local" ? "primary" : "subtle"}
                      onClick={() => setChoices((current) => ({ ...current, [item.rel]: "local" }))}
                    >
                      保留本机
                    </Button>
                    <Button
                      variant={choices[item.rel] === "remote" ? "primary" : "subtle"}
                      onClick={() => setChoices((current) => ({ ...current, [item.rel]: "remote" }))}
                    >
                      用云端
                    </Button>
                  </div>
                </div>
                <div className="mt-1 text-[11px] text-[var(--color-muted)]">
                  本机 {when(item.local_mtime)} · {item.local_bytes ?? 0} 字节 → 云端 {when(item.remote_mtime)} ·{" "}
                  {item.remote_bytes ?? 0} 字节
                </div>
              </div>
            ))}
          </div>
          <div className="flex justify-end gap-2">
            <Button variant="subtle" onClick={() => setConflicts([])}>
              稍后再说
            </Button>
            <Button variant="primary" onClick={() => void resolve()} disabled={busy !== null}>
              {busy === "resolve" ? "处理中…" : "应用选择"}
            </Button>
          </div>
        </div>
      </Modal>
    </Card>
  );
}
