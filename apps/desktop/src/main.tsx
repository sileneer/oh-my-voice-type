import { lazy, StrictMode, Suspense } from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";

const App = lazy(() => import("./App"));
const Pill = lazy(() => import("./Pill"));

async function boot() {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");
  document.documentElement.dataset.view = view === "pill" ? "pill" : "main";

  if (import.meta.env.DEV && params.get("mock") === "1") {
    const { installTauriMock } = await import("./lib/tauri-mock");
    installTauriMock();
  }

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <Suspense fallback={<div className="h-full w-full" aria-hidden="true" />}>
        {view === "pill" ? <Pill /> : <App />}
      </Suspense>
    </StrictMode>,
  );
}

void boot();
