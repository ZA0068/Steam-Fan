import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import * as theme from "./lib/theme.js";

// your look, as early as possible (a damaged one or --default-theme: the default)
theme.load().then((r) => theme.apply(r.theme));

mount(App, { target: document.getElementById("app") });
