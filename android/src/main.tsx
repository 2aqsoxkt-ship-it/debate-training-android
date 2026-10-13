import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./app/App";
import "./styles/global.css";
import "./platform/mobile.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
