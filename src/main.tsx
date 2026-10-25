import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import "react-grid-layout/css/styles.css";
import "./index.css";
import { App } from "./App";
import { queryClient } from "./lib/query";
import { isCaptureWindow } from "./lib/shell";

// The palette window is frameless and transparent: the page behind the card must
// not paint at all, otherwise the rounded card floats on a white rectangle.
// This runs before React mounts so there is no white flash either.
if (isCaptureWindow()) document.documentElement.classList.add("capture-window");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
