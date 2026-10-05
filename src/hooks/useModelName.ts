import { useTranslation } from "react-i18next";
import { modelNameParts } from "../lib/format";
import { useSettings } from "../store/settings";

/** Everyday and technical name of a catalog model (`標準`, `large-v3-turbo q5_0`). */
export function useModelName(modelId: string | null | undefined): { short: string; tech: string; full: string } | null {
  const { i18n } = useTranslation();
  const entry = useSettings((s) => s.models.find((m) => m.id === modelId));
  if (!modelId) return null;
  if (!entry) return { short: modelId, tech: "", full: modelId };
  const full = i18n.language === "ja" ? entry.nameJa : entry.nameEn;
  return { ...modelNameParts(full), full };
}
