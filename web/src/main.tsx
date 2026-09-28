import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createClient, restoreToken } from "./api/client";
import { App } from "./App";
import "./app.css";

const client = createClient(restoreToken());
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App client={client} />
  </StrictMode>,
);
