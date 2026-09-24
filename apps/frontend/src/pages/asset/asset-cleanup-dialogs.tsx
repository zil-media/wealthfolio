import TickerSearchInput from "@/components/ticker-search";
import type { SymbolSearchResult } from "@/lib/types";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@wealthfolio/ui/components/ui/alert-dialog";
import { Button } from "@wealthfolio/ui/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@wealthfolio/ui/components/ui/dialog";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useAssetManagement } from "./hooks/use-asset-management";

interface AssetRef {
  id: string;
  label: string;
}

interface DeleteAssetDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  asset: AssetRef;
  onDeleted: () => void;
}

export function DeleteAssetDialog({
  open,
  onOpenChange,
  asset,
  onDeleted,
}: DeleteAssetDialogProps) {
  const { t } = useTranslation();
  const { deleteAssetMutation } = useAssetManagement();

  const handleDelete = async () => {
    await deleteAssetMutation.mutateAsync(asset.id);
    onOpenChange(false);
    onDeleted();
  };

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t("asset:securities.delete_title")}</AlertDialogTitle>
          <AlertDialogDescription>
            {t("asset:securities.delete_description_named", { name: asset.label })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <p className="text-muted-foreground break-all font-mono text-xs">{asset.id}</p>
        <AlertDialogFooter>
          <AlertDialogCancel>{t("common:cancel")}</AlertDialogCancel>
          <AlertDialogAction
            onClick={(event) => {
              event.preventDefault();
              void handleDelete().catch(() => undefined);
            }}
            disabled={deleteAssetMutation.isPending}
            className="bg-destructive hover:bg-destructive/90 dark:text-foreground"
          >
            {deleteAssetMutation.isPending ? t("asset:securities.deleting") : t("common:delete")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

interface MergeAssetDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  source: AssetRef;
  activityCount: number;
  onMerged: (targetId: string) => void;
}

export function MergeAssetDialog({
  open,
  onOpenChange,
  source,
  activityCount,
  onMerged,
}: MergeAssetDialogProps) {
  const { t } = useTranslation();
  const { mergeAssetsMutation } = useAssetManagement();
  const [target, setTarget] = useState<SymbolSearchResult | undefined>();
  const [confirming, setConfirming] = useState(false);

  const targetId = target?.existingAssetId;
  const isSameAsset = targetId === source.id;
  const targetLabel = target ? target.longName || target.shortName || target.symbol : "";

  const handleOpenChange = (next: boolean) => {
    if (!next) {
      setTarget(undefined);
      setConfirming(false);
    }
    onOpenChange(next);
  };

  const handleMerge = async () => {
    if (!targetId || isSameAsset) return;
    await mergeAssetsMutation.mutateAsync({ sourceId: source.id, targetId });
    handleOpenChange(false);
    onMerged(targetId);
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("asset:profile.merge_title", { name: source.label })}</DialogTitle>
          <DialogDescription>{t("asset:profile.merge_description")}</DialogDescription>
        </DialogHeader>

        {!confirming ? (
          <div className="space-y-2">
            <TickerSearchInput
              existingOnly
              placeholder={t("asset:profile.merge_target_placeholder")}
              selectedResult={target}
              onSelectResult={(_symbol, result) => setTarget(result)}
              onClear={() => setTarget(undefined)}
            />
            {isSameAsset && (
              <p className="text-destructive text-sm">{t("asset:profile.merge_same_asset")}</p>
            )}
          </div>
        ) : (
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            <dt className="text-muted-foreground">{t("asset:profile.merge_source")}</dt>
            <dd className="min-w-0">
              <div>{source.label}</div>
              <div className="text-muted-foreground break-all font-mono text-xs">{source.id}</div>
            </dd>
            <dt className="text-muted-foreground">{t("asset:profile.merge_target")}</dt>
            <dd className="min-w-0">
              <div>{targetLabel}</div>
              <div className="text-muted-foreground break-all font-mono text-xs">{targetId}</div>
            </dd>
            <dt className="text-muted-foreground">{t("asset:profile.merge_activity_count")}</dt>
            <dd className="font-medium">{activityCount}</dd>
          </dl>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => handleOpenChange(false)}>
            {t("common:cancel")}
          </Button>
          {!confirming ? (
            <Button disabled={!targetId || isSameAsset} onClick={() => setConfirming(true)}>
              {t("asset:profile.merge_review")}
            </Button>
          ) : (
            <Button
              variant="destructive"
              disabled={mergeAssetsMutation.isPending}
              onClick={() => void handleMerge().catch(() => undefined)}
            >
              {mergeAssetsMutation.isPending
                ? t("asset:profile.merging")
                : t("asset:profile.merge_confirm")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
