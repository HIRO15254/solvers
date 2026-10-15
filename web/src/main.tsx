import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { adoptTokenFromLocation } from "./api/client";
import { App } from "./App";
import "./styles.css";

adoptTokenFromLocation();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
