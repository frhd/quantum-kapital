import React from "react"
import ReactDOM from "react-dom/client"
import "@fontsource/inter/400.css"
import "@fontsource/inter/500.css"
import "@fontsource/inter/600.css"
import "@fontsource/inter/700.css"
import "@fontsource/jetbrains-mono/400.css"
import "@fontsource/jetbrains-mono/500.css"
import App from "./app/App"
import "./styles/index.css"

const storedTheme = localStorage.getItem("qk-theme")
const initialTheme = storedTheme === "light" ? "light" : "dark"
document.documentElement.classList.toggle("dark", initialTheme === "dark")

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
