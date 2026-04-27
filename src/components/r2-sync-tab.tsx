"use client";

import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { LuEye, LuEyeOff, LuRefreshCw } from "react-icons/lu";
import { LoadingButton } from "@/components/loading-button";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import type { R2SyncResult, R2SyncSettings } from "@/types";

const INTERVAL_OPTIONS = [
  { value: 15, label: "15 minutes" },
  { value: 30, label: "30 minutes" },
  { value: 60, label: "1 hour" },
  { value: 360, label: "6 hours" },
  { value: 1440, label: "24 hours" },
];

function formatLastSync(timestamp?: number): string {
  if (!timestamp) return "Never";
  return new Date(timestamp * 1000).toLocaleString();
}

export function R2SyncTab() {
  const { t } = useTranslation();

  const [accountId, setAccountId] = useState("");
  const [bucketName, setBucketName] = useState("");
  const [accessKeyId, setAccessKeyId] = useState("");
  const [secretAccessKey, setSecretAccessKey] = useState("");
  const [intervalMinutes, setIntervalMinutes] = useState(60);
  const [enabled, setEnabled] = useState(false);
  const [lastSync, setLastSync] = useState<number | undefined>(undefined);
  const [lastSyncError, setLastSyncError] = useState<string | undefined>(
    undefined,
  );

  const [showSecret, setShowSecret] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [isSyncing, setIsSyncing] = useState(false);

  const [connectionStatus, setConnectionStatus] = useState<
    "unknown" | "connected" | "error"
  >("unknown");

  const loadSettings = useCallback(async () => {
    setIsLoading(true);
    try {
      const settings = await invoke<R2SyncSettings>("get_r2_sync_settings");
      setAccountId(settings.account_id ?? "");
      setBucketName(settings.bucket_name ?? "");
      setAccessKeyId(settings.access_key_id ?? "");
      setSecretAccessKey(settings.secret_access_key ?? "");
      setIntervalMinutes(settings.interval_minutes || 60);
      setEnabled(settings.enabled ?? false);
      setLastSync(settings.last_sync);
      setLastSyncError(settings.last_sync_error);
    } catch (error) {
      console.error("Failed to load R2 sync settings:", error);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadSettings();
  }, [loadSettings]);

  const hasFullConfig =
    accountId.trim() &&
    bucketName.trim() &&
    accessKeyId.trim() &&
    secretAccessKey.trim();

  const handleTestConnection = useCallback(async () => {
    if (!hasFullConfig) {
      showErrorToast(t("sync.r2.fillAllFields"));
      return;
    }
    setIsTesting(true);
    try {
      await invoke("test_r2_connection", {
        accountId: accountId.trim(),
        bucketName: bucketName.trim(),
        accessKeyId: accessKeyId.trim(),
        secretAccessKey: secretAccessKey.trim(),
      });
      setConnectionStatus("connected");
      showSuccessToast(t("sync.r2.connectionSuccess"));
    } catch (error) {
      setConnectionStatus("error");
      showErrorToast(String(error));
    } finally {
      setIsTesting(false);
    }
  }, [accountId, bucketName, accessKeyId, secretAccessKey, hasFullConfig, t]);

  const handleSave = useCallback(async () => {
    if (!hasFullConfig) {
      showErrorToast(t("sync.r2.fillAllFields"));
      return;
    }
    setIsSaving(true);
    try {
      const settings: R2SyncSettings = {
        enabled: true,
        account_id: accountId.trim(),
        bucket_name: bucketName.trim(),
        access_key_id: accessKeyId.trim(),
        secret_access_key: secretAccessKey.trim(),
        interval_minutes: intervalMinutes,
      };
      await invoke("save_r2_sync_settings", { settings });
      setEnabled(true);
      showSuccessToast(t("sync.r2.settingsSaved"));
    } catch (error) {
      showErrorToast(String(error));
    } finally {
      setIsSaving(false);
    }
  }, [
    accountId,
    bucketName,
    accessKeyId,
    secretAccessKey,
    intervalMinutes,
    hasFullConfig,
    t,
  ]);

  const handleSyncNow = useCallback(async () => {
    setIsSyncing(true);
    try {
      const result = await invoke<R2SyncResult>("trigger_r2_sync_now");
      setLastSync(result.synced_at);
      setLastSyncError(undefined);
      showSuccessToast(
        t("sync.r2.syncSuccess", {
          profiles: result.profiles_count,
          proxies: result.proxies_count,
          groups: result.groups_count,
        }),
      );
    } catch (error) {
      showErrorToast(String(error));
    } finally {
      setIsSyncing(false);
    }
  }, [t]);

  const handleDisconnect = useCallback(async () => {
    setIsSaving(true);
    try {
      await invoke("disconnect_r2_sync");
      setEnabled(false);
      setAccountId("");
      setBucketName("");
      setAccessKeyId("");
      setSecretAccessKey("");
      setConnectionStatus("unknown");
      setLastSync(undefined);
      setLastSyncError(undefined);
      showSuccessToast(t("sync.r2.disconnected"));
    } catch (error) {
      showErrorToast(String(error));
    } finally {
      setIsSaving(false);
    }
  }, [t]);

  if (isLoading) {
    return (
      <div className="flex justify-center py-8">
        <div className="w-6 h-6 rounded-full border-2 border-current animate-spin border-t-transparent" />
      </div>
    );
  }

  return (
    <div className="grid gap-4 py-4">
      <p className="text-sm text-muted-foreground">
        {t("sync.r2.description")}
      </p>

      {enabled && (
        <div className="rounded-md border border-border bg-success/10 p-3 space-y-1">
          <div className="flex items-center gap-2 text-sm">
            <div className="w-2 h-2 rounded-full bg-success shrink-0" />
            <span className="font-medium">{t("sync.r2.active")}</span>
          </div>
          <div className="text-xs text-muted-foreground pl-4">
            {t("sync.r2.lastSync")}: {formatLastSync(lastSync)}
          </div>
          {lastSyncError && (
            <div className="text-xs text-destructive pl-4">{lastSyncError}</div>
          )}
        </div>
      )}

      <div className="space-y-2">
        <Label htmlFor="r2-account-id">{t("sync.r2.accountId")}</Label>
        <Input
          id="r2-account-id"
          placeholder={t("sync.r2.accountIdPlaceholder")}
          value={accountId}
          onChange={(e) => {
            setAccountId(e.target.value);
          }}
          autoComplete="off"
          spellCheck={false}
        />
      </div>

      <div className="space-y-2">
        <Label htmlFor="r2-bucket">{t("sync.r2.bucketName")}</Label>
        <Input
          id="r2-bucket"
          placeholder={t("sync.r2.bucketNamePlaceholder")}
          value={bucketName}
          onChange={(e) => {
            setBucketName(e.target.value);
          }}
          autoComplete="off"
          spellCheck={false}
        />
      </div>

      <div className="space-y-2">
        <Label htmlFor="r2-access-key">{t("sync.r2.accessKeyId")}</Label>
        <Input
          id="r2-access-key"
          placeholder={t("sync.r2.accessKeyIdPlaceholder")}
          value={accessKeyId}
          onChange={(e) => {
            setAccessKeyId(e.target.value);
          }}
          autoComplete="off"
          spellCheck={false}
        />
      </div>

      <div className="space-y-2">
        <Label htmlFor="r2-secret-key">{t("sync.r2.secretAccessKey")}</Label>
        <div className="relative">
          <Input
            id="r2-secret-key"
            type={showSecret ? "text" : "password"}
            placeholder={t("sync.r2.secretAccessKeyPlaceholder")}
            value={secretAccessKey}
            onChange={(e) => {
              setSecretAccessKey(e.target.value);
            }}
            className="pr-10"
            autoComplete="new-password"
            spellCheck={false}
          />
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => {
                  setShowSecret(!showSecret);
                }}
                className="absolute right-3 top-1/2 p-1 rounded-sm transition-colors transform -translate-y-1/2 hover:bg-accent"
                aria-label={showSecret ? "Hide key" : "Show key"}
              >
                {showSecret ? (
                  <LuEyeOff className="w-4 h-4 text-muted-foreground hover:text-foreground" />
                ) : (
                  <LuEye className="w-4 h-4 text-muted-foreground hover:text-foreground" />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent>
              {showSecret ? t("sync.r2.hideKey") : t("sync.r2.showKey")}
            </TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div className="space-y-2">
        <Label htmlFor="r2-interval">{t("sync.r2.syncInterval")}</Label>
        <Select
          value={String(intervalMinutes)}
          onValueChange={(v) => {
            setIntervalMinutes(Number(v));
          }}
        >
          <SelectTrigger id="r2-interval">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {INTERVAL_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={String(opt.value)}>
                {t(`sync.r2.intervals.${String(opt.value)}`, {
                  defaultValue: opt.label,
                })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("sync.r2.intervalNote")}
        </p>
      </div>

      {connectionStatus === "connected" && (
        <div className="flex gap-2 items-center text-sm text-muted-foreground">
          <div className="w-2 h-2 rounded-full bg-success" />
          {t("sync.r2.connectionSuccess")}
        </div>
      )}
      {connectionStatus === "error" && (
        <div className="flex gap-2 items-center text-sm text-muted-foreground">
          <div className="w-2 h-2 rounded-full bg-destructive" />
          {t("sync.r2.connectionFailed")}
        </div>
      )}

      <div className="flex gap-2 flex-wrap pt-1">
        <Button
          variant="outline"
          onClick={() => void handleTestConnection()}
          disabled={isTesting || !hasFullConfig}
          className="flex-1"
        >
          {isTesting ? t("sync.r2.testing") : t("sync.r2.testConnection")}
        </Button>

        {enabled && (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="outline"
                size="icon"
                onClick={() => void handleSyncNow()}
                disabled={isSyncing}
              >
                <LuRefreshCw
                  className={`w-4 h-4 ${isSyncing ? "animate-spin" : ""}`}
                />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t("sync.r2.syncNow")}</TooltipContent>
          </Tooltip>
        )}
      </div>

      <div className="flex gap-2">
        {enabled && (
          <Button
            variant="outline"
            onClick={() => void handleDisconnect()}
            disabled={isSaving}
            className="flex-1"
          >
            {t("sync.r2.disconnect")}
          </Button>
        )}
        <LoadingButton
          onClick={() => void handleSave()}
          isLoading={isSaving}
          disabled={!hasFullConfig}
          className="flex-1"
        >
          {t("sync.r2.save")}
        </LoadingButton>
      </div>
    </div>
  );
}
