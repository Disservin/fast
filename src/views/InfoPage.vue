<script setup lang="ts"></script>

<template>
  <main>
    <div>
      <h1 class="ma-10">
        Information
      </h1>
      <div class="content ma-15">
        <p>
          Fast by Disservin 2023. This is an open source chess gui.
          <br>
          You can find the source code on
          <a
            href="https://github.com/Disservin/fast-chess"
            target="_blank"
          >GitHub</a>.
        </p>

        <div>
          <br>
          Many thanks to the following projects that made this possible:
          <ul>
            <li>
              <a
                href="https://github.com/tauri-apps/tauri"
                target="_blank"
              >
                Tauri
              </a>
            </li>
            <li>
              <a
                href="https://github.com/lichess-org/chessground"
                target="_blank"
              >
                Chessground
              </a>
            </li>
            <li>
              <a
                href="https://github.com/jhlywa/chess.js"
                target="_blank"
              >
                Chess.js
              </a>
            </li>
            <li>
              <a
                href="https://github.com/vuejs/vue"
                target="_blank"
              >
                Vue.js
              </a>
            </li>
            <li>
              <a
                href="https://github.com/microsoft/TypeScript"
                target="_blank"
              >
                TypeScript
              </a>
            </li>
            <li>
              <a
                href="https://github.com/vitejs/vite"
                target="_blank"
              >
                Vite
              </a>
            </li>
          </ul>
        </div>
      </div>
    </div>
  </main>
</template>

<style scoped>
@import "@/assets/styles/variables.css";

main {
  margin-left: 5rem;
  color: var(--text-primary);
}

a {
  color: var(--link-color);
}

a:hover {
  color: var(--link-color-hover);
}

ul {
  list-style-type: none;
}
</style>
