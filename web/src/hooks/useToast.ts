import { useState, useCallback, useRef } from "react";

interface Toast {
  message: string;
  isError: boolean;
  visible: boolean;
}

export function useToast() {
  const [toast, setToast] = useState<Toast>({ message: "", isError: false, visible: false });
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);

  const show = useCallback((message: string, isError = false) => {
    clearTimeout(timer.current);
    setToast({ message, isError, visible: true });
    timer.current = setTimeout(() => {
      setToast((t) => ({ ...t, visible: false }));
    }, 3000);
  }, []);

  return { toast, show };
}
