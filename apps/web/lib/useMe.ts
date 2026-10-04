"use client";

import { useCallback, useEffect, useState } from "react";
import { api, ApiFailure, type Me } from "./api";

/** Loads /v1/me; sends signed-out visitors to sign-in. */
export function useMe() {
  const [me, setMe] = useState<Me | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setMe(await api<Me>("/v1/me"));
      setError(null);
    } catch (err) {
      if (err instanceof ApiFailure && err.status === 401) {
        window.location.replace(`/signin?next=${encodeURIComponent(window.location.pathname)}`);
        return;
      }
      setError("Couldn't load your account. Try again.");
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { me, error, reload };
}
