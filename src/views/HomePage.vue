<script setup lang="ts"></script>

<template>
  <main>
    <div class="screen">
      <div class="welcome">
        <span>Welcome!</span>
      </div>
      <div class="content">
        <span>
          This is a work in progress Chess GUI. You can contribute to the
          project on
          <a
            href="https://github.com/Disservin/fast-chess"
            target="_blank"
          >Github</a>.
        </span>
      </div>
    </div>
  </main>
</template>

<style scoped>
@import "@/assets/styles/variables.css";

main {
  margin-left: 5rem;
}

.screen {
  display: flex;
  justify-content: center;
  flex-direction: column;
}

.welcome {
  margin-top: 30vh;
  text-align: center;
  font-size: 5rem;
}

.content {
  margin-top: 10vh;
  text-align: center;
  font-size: 1.5rem;
}

.welcome span {
  color: var(--text-primary);
}

.content span {
  color: var(--text-secondary);
}

a {
  color: var(--link-color);
}

a:hover {
  color: var(--link-color-hover);
}
</style>
