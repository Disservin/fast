<template>
  <main>
    <div class="engine-table">
      <div
        class="engine-column"
        v-for="(engine, index) in engines"
        :key="index"
        :class="{ active: engine.use }"
      >
        <div v-if="editingIndex !== index || editedEngine === null">
          <button class="remove-button" @click="removeEngine(index)">
            Remove
          </button>
          <button class="edit-button" @click="editEngine(index)">Edit</button>
          <button class="use-button" @click="useEngine(index)">Use</button>
        </div>
        <div>
          <h2>{{ engine.name }}</h2>
          <div v-if="editingIndex !== index || editedEngine === null">
            <p><u>UCI Options</u></p>

            <div v-for="(value, key) in engine.settings" :key="key">
              <p>{{ key }}: {{ value }}</p>
            </div>
            <p><u>Path</u></p>
            {{ engine.path }}
          </div>
        </div>
        <div v-if="editingIndex === index && editedEngine !== null">
          <form @submit.prevent="saveEdit">
            <label style="font-weight: bold"
              >Name: <br />
              <input v-model="editedEngine.name"
            /></label>
            <br />
            <label style="font-weight: bold"
              >Path: <br />
              <button @click="selectFile(index)">Choose File</button>
            </label>
            <br />
            <label style="font-weight: bold">Settings:</label>
            <br />
            <div v-for="(value, key) in editedEngine.settings" :key="key">
              <label>{{ key }}: <br /></label>
              <input v-model="editedEngine?.settings[key]" />
            </div>
            <div class="setting-buttons">
              <button type="button" @click="cancelEdit">Cancel</button>
              <button type="submit">Save</button>
            </div>
          </form>
        </div>
      </div>
    </div>
    <button class="add-engine-button" @click="addEngine()">+</button>
  </main>
</template>

<script lang="ts">
import { invoke } from "@tauri-apps/api";
import { open } from "@tauri-apps/api/dialog";

import { defineComponent } from "vue";

interface Engine {
  name: string;
  path: string;
  use: boolean;
  settings: Record<string, unknown>;
}

export default defineComponent({
  name: "Engines",
  data() {
    const enginesData = localStorage.getItem("engines");
    const engines = enginesData ? JSON.parse(enginesData) : [];

    return {
      engines,
      editingIndex: null as number | null,
      editedEngine: null as Engine | null,
    };
  },
  mounted() {},
  methods: {
    async selectFile(index: number) {
      // Open a file dialog and get the path of the selected file
      // we cant use html input type file because we will get a fakepath
      const result = await open({
        filters: [{ name: "All Files", extensions: ["*"] }],
      });

      // update the path of the engine
      if (result && !Array.isArray(result)) {
        // we need to use the index to update the correct engine
        this.editingIndex = index;
        this.engines[index]!.path = result;
      }
    },
    removeEngine(index: number) {
      this.engines.splice(index, 1);
      localStorage.setItem("engines", JSON.stringify(this.engines));
    },
    editEngine(index: number) {
      // Set the editing index and copy the engine to edit to a new object
      this.editingIndex = index;
      this.editedEngine = { ...this.engines[index] };
    },
    cancelEdit() {
      // Reset the editing state
      this.editingIndex = null;
      this.editedEngine = null;
    },
    saveEdit() {
      if (this.editingIndex === null || this.editedEngine === null) {
        return;
      }
      // Update the engine in the engines array
      this.engines.splice(this.editingIndex, 1, this.editedEngine);
      // Reset the editing state
      this.editingIndex = null;
      this.editedEngine = null;

      localStorage.setItem("engines", JSON.stringify(this.engines));
    },
    useEngine(index: number) {
      this.engines.forEach((engine: Engine) => {
        engine.use = false;
      });
      this.engines[index].use = true;

      localStorage.setItem("engines", JSON.stringify(this.engines));
    },
    addEngine() {
      const newEngine: Engine = {
        name: "New Engine ",
        path: "empty",
        use: false,
        settings: { hash: 0 },
      };

      // Add the new engine object to the array
      this.engines.push(newEngine);

      localStorage.setItem("engines", JSON.stringify(this.engines));
    },
  },
});
</script>

<style scoped>
@import "../assets/styles/variables.css";

.engine-table {
  display: flex;
  flex-direction: column;
  position: absolute;
  height: auto;
  bottom: 0;
  top: 0;
  left: 0;
  right: 0;
  margin: 20px;
  margin-left: 6rem;
  margin-bottom: 5%;

  background-color: var(--bg-secondary);

  overflow: scroll;
  overflow-x: hidden;
}

.engine-column {
  color: var(--text-secondary);
  background-color: var(--bg-tertiary);
  padding: 10px;
  margin: 10px;
  border-radius: 5px;
}

.engine-column.active {
  filter: grayscale(0%) opacity(1);
  background-color: #3700b3;
}

.engine-table::-webkit-scrollbar {
  width: 0.25rem;
}

.engine-table::-webkit-scrollbar-track {
  background: #1e1e24;
}

.engine-table::-webkit-scrollbar-thumb {
  background: #6649b8;
}

.engine-column input {
  background-color: var(--button-bg);
  color: var(--button-text);
  border-radius: 3px;
  padding: 5px 10px;
  border: none;
}

.engine-column button {
  background-color: var(--button-bg);
  color: var(--button-text);
  border-radius: 3px;
  padding: 5px 10px;
  border: none;
  cursor: pointer;
}

.engine-column button:hover {
  background-color: var(--button-bg-hover);
  color: var(--button-text-hover);
}

.engine-column button:active {
  background-color: var(--button-bg-active);
  color: var(--button-text-active);
}

.engine-column.active button {
  background-color: white;
  color: #000000;
}

.engine-column.active h2 {
  color: var(--button-text);
}

.edit-button,
.use-button {
  margin-left: 5px;
}

.setting-buttons button {
  margin: 5px;
}

.add-engine-button {
  position: absolute;
  bottom: 20px;
  left: 50%;
  transform: translateX(-50%);
  width: 60px;
  height: 60px;
  border-radius: 50%;
  background-color: #007bff;
  color: #fff;
  font-size: 30px;
  line-height: 1;
  border: none;
  box-shadow: 0px 3px 8px rgba(0, 0, 0, 0.2);
  cursor: pointer;
}

input {
  padding: 0.5rem;
  border-radius: 0.25rem;
  border: 1px solid gray;
  font-size: 1rem;
}
</style>
