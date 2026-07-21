import { useCallback, useEffect, useState } from "react";

import { esList, esView } from "./engine";
import type { EsListItem } from "./types";

/**
 * Interview ES base: list + optional body load via es.view(id).
 * Soft-fails when Python sidecar is unavailable (paste-only still works).
 */
export function useInterviewEsBase() {
  const [esId, setEsId] = useState("");
  const [esText, setEsText] = useState("");
  const [items, setItems] = useState<EsListItem[]>([]);

  useEffect(() => {
    let cancelled = false;
    void esList()
      .then((list) => {
        if (!cancelled) setItems(list);
      })
      .catch(() => {
        if (!cancelled) setItems([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onEsIdChange = useCallback(async (id: string) => {
    setEsId(id);
    if (!id) {
      setEsText("");
      return;
    }
    try {
      const view = await esView(id);
      if (view.exists && typeof view.body === "string") {
        setEsText(view.body);
      }
    } catch {
      // Engine unavailable — keep selection; user can paste.
    }
  }, []);

  const onEsTextChange = useCallback((text: string) => {
    setEsText(text);
  }, []);

  return {
    esId,
    esText,
    items,
    onEsIdChange,
    onEsTextChange,
  };
}
