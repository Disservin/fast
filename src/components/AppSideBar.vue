<script setup lang="ts"></script>

<template>
  <nav class="navbar">
    <ul class="navbar-nav">
      <li class="logo">
        <router-link
          to="/"
          class="nav-link"
        >
          <span class="link-text logo-text">fast</span>
        </router-link>
      </li>
      <li class="nav-item">
        <router-link
          to="/analysis"
          class="nav-link"
          exact-active-class="active"
        >
          <font-awesome-icon icon="fa-brands fa-searchengin" />
          <span class="link-text">Analysis</span>
        </router-link>
      </li>
      <li class="nav-item">
        <router-link
          to="/engines"
          class="nav-link"
          exact-active-class="active"
        >
          <font-awesome-icon icon="fa-solid fa-list" />
          <span class="link-text">Engines</span>
        </router-link>
      </li>
      <li class="nav-item">
        <router-link
          to="/info"
          class="nav-link"
          exact-active-class="active"
        >
          <font-awesome-icon icon="fa-solid fa-circle-info" />
          <span class="link-text">Info</span>
        </router-link>
      </li>
      <li class="nav-item">
        <router-link
          to="/settings"
          class="nav-link"
          exact-active-class="active"
        >
          <font-awesome-icon icon="fa-solid fa-gear" />
          <span class="link-text">Settings</span>
        </router-link>
      </li>
    </ul>
  </nav>
</template>

<style>
@import "@/assets/styles/variables.css";

:root {
  font-size: 16px;
  font-family: "Roboto", sans-serif;
}

body {
  background-color: var(--bg-primary);
  margin: 0;
  padding: 0;
}

::-webkit-scrollbar {
  width: 0.25rem;
}

::-webkit-scrollbar-track {
  background: #1e1e24;
}

::-webkit-scrollbar-thumb {
  background: #6649b8;
}

.navbar {
  position: fixed;
  background-color: var(--bg-primary);
  transition: width 600ms ease;
  overflow: hidden;
  z-index: 3;
}

.navbar-nav {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  height: 100%;
  background-color: var(--bg-secondary);
}

.nav-item {
  width: 100%;
}

.nav-item:last-child {
  margin-top: auto;
}

.nav-link {
  display: flex;
  align-items: center;
  height: 5rem;
  color: var(--text-primary);
  text-decoration: none;
  filter: grayscale(100%) opacity(0.7);
  transition: 200ms;
}

.nav-link:hover {
  filter: grayscale(0%) opacity(1);
  background: var(--bg-secondary);
  color: var(--text-secondary);
}

.nav-link.active {
  filter: grayscale(0%) opacity(1);
  background: #6649b8;
}

.link-text {
  display: none;
  margin-left: 1rem;
}

.nav-link svg {
  width: 2rem;
  min-width: 2rem;
  margin: 0 1.5rem;
}

.fa-primary {
  color: #ff7eee;
}

.fa-secondary {
  color: #df49a6;
}

.fa-primary,
.fa-secondary {
  transition: var(--transition-speed);
}

.logo {
  font-weight: bold;
  text-transform: uppercase;
  margin-bottom: 1rem;
  text-align: center;
  color: var(--text-secondary);
  background: var(--bg-secondary);
  font-size: 1.5rem;
  letter-spacing: 0.3ch;
  width: 100%;
}

.logo svg {
  transform: rotate(0deg);
  transition: var(--transition-speed);
}

.logo-text {
  display: inline;
  position: absolute;
  left: -999px;
  transition: var(--transition-speed);
}

.navbar:hover .logo svg {
  transform: rotate(-180deg);
}

/* Small screens */
@media only screen and (max-width: 600px) {
  .navbar {
    bottom: 0;
    width: 100vw;
    height: 5rem;
  }

  .logo {
    display: none;
  }

  .navbar-nav {
    flex-direction: row;
  }

  .nav-link {
    justify-content: center;
  }

  main {
    margin: 0;
  }
}

/* Large screens */
@media only screen and (min-width: 600px) {
  .navbar {
    top: 0;
    width: 5rem;
    height: 100vh;
  }

  .navbar:hover {
    width: 16rem;
  }

  .navbar:hover .link-text {
    display: inline;
  }

  .navbar:hover .logo svg {
    margin-left: 11rem;
  }

  .navbar:hover .logo-text {
    left: 0px;
  }
}

.theme-icon {
  display: none;
}
</style>
