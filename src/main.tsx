import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import "./index.css";
import { App } from "./App";
import { queryClient } from "./lib/query";

const isCapture = window.location.hash.startsWith("#/capture");
if (isCapture) {
  document.documentElement.classList.add("capture-window");
  document.body.classList.add("capture-window");
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
