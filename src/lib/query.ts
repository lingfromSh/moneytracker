import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      retry: 0,
      refetchOnWindowFocus: false,
    },
  },
});

/** The ledger changed on disk: drop every cached projection. */
export function invalidateAll(): void {
  void queryClient.invalidateQueries();
}
