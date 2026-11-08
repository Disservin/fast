import "./style.css";
import analyse from "../../docs/screenshots/analyse.png";
import database from "../../docs/screenshots/database.png";
import train from "../../docs/screenshots/train.png";
import play from "../../docs/screenshots/play.png";
import tournament from "../../docs/screenshots/tournament.png";
import settings from "../../docs/screenshots/settings.png";

const screens = [
  {
    name: "Analyse",
    image: analyse,
    description:
      "Compare UCI engines, explore variations and inspect evaluation, time and depth.",
    alt: "Analyse with a chessboard, Stockfish and Lc0 lines, scoresheet and evaluation graph",
  },
  {
    name: "Database",
    image: database,
    description:
      "Filter your games, find positions and review where you left your repertoire.",
    alt: "Database with game filters, a game table and board preview",
  },
  {
    name: "Train",
    image: train,
    description:
      "Practise opening lines with spaced repetition and drills from your own games.",
    alt: "Repertoire training with a chessboard, scheduled lines and grading controls",
  },
  {
    name: "Play",
    image: play,
    description:
      "Play an engine at your chosen strength, with clocks and optional premoves.",
    alt: "An engine game with a chessboard, clocks and scoresheet",
  },
  {
    name: "Tournament",
    image: tournament,
    description:
      "Run engine matches with fastchess and follow the games on live boards.",
    alt: "Live engine tournament overview with six green chessboards",
  },
  {
    name: "Settings",
    image: settings,
    description:
      "Manage engines, option profiles, appearance and keyboard shortcuts.",
    alt: "Engine settings showing installed engines and their UCI options",
  },
];
const tablist = document.querySelector(".screen-tabs");

/** Show the chosen workspace screen and keep its accessible tab state in sync. */
function selectScreen(index, focus = false) {
  const screen = screens[index];
  tablist.querySelectorAll("button").forEach((tab, i) => {
    tab.setAttribute("aria-selected", String(i === index));
    tab.tabIndex = i === index ? 0 : -1;
    if (i === index && focus) tab.focus();
  });
  document.querySelector(".screen-caption .text-link").href =
    `./docs.html#${screen.name === "Tournament" ? "tournament" : screen.name.toLowerCase()}`;
  const image = document.querySelector("#screenshot");
  image.src = screen.image;
  image.alt = screen.alt;
  const link = document.querySelector("#screenshot-link");
  link.href = screen.image;
  link.setAttribute("aria-label", `Open full-size ${screen.name} screenshot`);
  document.querySelector("#screen-title").textContent = screen.name;
  document.querySelector("#screen-description").textContent =
    screen.description;
  document
    .querySelector("#screen-panel")
    .setAttribute("aria-labelledby", `tab-${index}`);
}

screens.forEach((screen, index) => {
  const tab = document.createElement("button");
  tab.type = "button";
  tab.id = `tab-${index}`;
  tab.textContent = screen.name;
  tab.setAttribute("role", "tab");
  tab.setAttribute("aria-controls", "screen-panel");
  tab.addEventListener("click", () => selectScreen(index));
  tab.addEventListener("keydown", (event) => {
    const targets = {
      ArrowRight: (index + 1) % screens.length,
      ArrowLeft: (index + screens.length - 1) % screens.length,
      Home: 0,
      End: screens.length - 1,
    };
    if (Object.hasOwn(targets, event.key)) {
      event.preventDefault();
      selectScreen(targets[event.key], true);
    }
  });
  tablist.append(tab);
});
selectScreen(0);
