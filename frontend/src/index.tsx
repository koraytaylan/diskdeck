/**
 * @file Application Entry Point
 *
 * Bootstraps the SolidJS application by mounting the `<App />` component
 * into the `#root` DOM element. Imports the global CSS stylesheet which
 * defines theme variables and base styles.
 *
 * The `@refresh reload` pragma tells Vite's HMR to do a full reload
 * instead of a hot swap when this file changes (since it is the mount point).
 *
 * @module index
 */

/* @refresh reload */
import { render } from "solid-js/web";
import "./assets/styles/global.css";
import App from "./App";

const root = document.getElementById("root");

if (!root) throw new Error("Root element not found");

render(() => <App />, root);
