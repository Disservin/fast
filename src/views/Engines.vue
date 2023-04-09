<script lang="ts">
import { defineComponent } from "vue";
import { open } from "@tauri-apps/api/dialog";

import ChessProcess from "../ts/ChessProcess";
import type { Option, Engine } from "@/ts/Types";

export default defineComponent({
  name: "Engines",
  data() {
    return {
      chessProcess: null as ChessProcess | null,
      engines: [] as Engine[],
      editingIndex: null as number | null,
      editedEngine: null as Engine | null,
    };
  },
  mounted() {
    const enginesData = localStorage.getItem("engines");

    if (enginesData) {
      this.engines = JSON.parse(enginesData) as Engine[];
    }
  },
  methods: {
    // Update an engine setting with a file path
    async selectFile(index: number, key: any) {
      if (this.engines.length === 0 || index > this.engines.length) return;

      const selected = await open();

      if (!Array.isArray(selected) && selected !== null) {
        this.engines[index].settings[key].value = selected;
      }

      localStorage.setItem("engines", JSON.stringify(this.engines));
    },
    async selectEngine(index: number) {
      // Open a file dialog and get the path of the selected file
      // we cant use html input type file because we will get a fakepath
      const result = await open();

      if (result && !Array.isArray(result)) {
        this.chessProcess = new ChessProcess(result, (line) => {
          if (line.trim() == "uciok") {
            this.chessProcess?.sendQuit();
            return;
          }

          this.editingIndex = index;
          this.engines[index]!.path = result;

          let settings: Option[] = this.engines[index].settings || [];

          line = line.replace(/^\s+|\s+$/g, "");
          line = line.trim();

          if (line.startsWith("id name")) {
            this.engines[index]!.name = line.replace("id name ", "");
          } else if (line.startsWith("option name")) {
            const parts = line.split(" ");

            let option: Option = {
              name: "",
              type: "",
              default: "",
              min: 0,
              max: 0,
              value: "",
            };

            let length = 1;

            while (length + 1 < parts.length && parts[++length] !== "type") {
              option.name += " " + parts[length];
            }

            if (length + 1 < parts.length && parts[length++] === "type") {
              option.type = parts[length];
            }

            if (length + 1 < parts.length && parts[++length] === "default") {
              length++;
              option.default = option.value = parts[length];
            }

            if (length + 1 < parts.length && parts[++length] === "min") {
              length++;
              option.min = Number(parts[length]);
            }

            if (length + 1 < parts.length && parts[++length] === "max") {
              length++;
              option.max = Number(parts[length]);
            }

            option.name = option.name.trim();

            // overwrite types
            if (option.name === "SyzygyPath") option.type = "file";
            if (option.name === "EvalFile") option.type = "file";
            if (option.type !== "button") settings.push(option);
          }

          // we need to use the index to update the correct engine
          this.engines[index]!.settings = settings;

          localStorage.setItem("engines", JSON.stringify(this.engines));
        });

        await this.chessProcess.start();
        this.chessProcess.write("uci\n");
      }
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
    removeEngine(index: number) {
      this.engines.splice(index, 1);
      localStorage.setItem("engines", JSON.stringify(this.engines));
    },
    addEngine() {
      const option = {
        name: "",
        type: "",
        default: "0",
        min: 0,
        max: 0,
        value: "0",
      };

      const newEngine: Engine = {
        name: "New Engine ",
        path: "empty",
        use: false,
        settings: [option],
      };

      // Add the new engine object to the array
      this.engines.push(newEngine);

      localStorage.setItem("engines", JSON.stringify(this.engines));

      this.selectEngine(this.engines.length - 1);
    },
    useEngine(index: number) {
      this.engines.forEach((engine: Engine) => {
        engine.use = false;
      });
      this.engines[index].use = true;

      const engine = this.engines[0];
      this.engines[0] = this.engines[index];
      this.engines[index] = engine;

      localStorage.setItem("engines", JSON.stringify(this.engines));
    },
  },
});
</script>

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

            <div class="options">
              <div class="option" v-for="option in engine.settings">
                <div v-if="option && option.name != ''">
                  <div class="option-name">{{ option.name }}</div>
                  <div class="option-value">
                    <input
                      v-if="option.type == 'string' || option.type == 'file'"
                      v-model="option.value"
                      disabled
                    />
                    <input
                      v-if="option.type == 'spin'"
                      v-model="option.value"
                      disabled
                    />
                    <input
                      v-if="option.type == 'check'"
                      v-model="option.value"
                      type="checkbox"
                      disabled
                    />
                  </div>
                </div>
              </div>
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
              <button @click="selectEngine(index)">Choose Engine</button>
            </label>

            <br />
            <label style="font-weight: bold">Settings:</label>
            <br />

            <div class="options">
              <div
                class="option"
                v-for="(value, key) in editedEngine.settings"
                :key="key"
              >
                <div v-if="value.name !== ''">
                  <div class="option-name">
                    <label>{{ value.name }}:</label>
                  </div>
                  <div class="option-value">
                    <input
                      v-if="value.type == 'string' || value.type == 'file'"
                      v-model="editedEngine.settings[key].value"
                    />
                    <button
                      v-if="value.type == 'file'"
                      @click="selectFile(index, key)"
                      style="margin-left: 10px"
                    >
                      ...
                    </button>
                    <input
                      v-if="value.type == 'spin'"
                      v-model="editedEngine.settings[key].value"
                    />
                    <input
                      v-if="value.type == 'check'"
                      v-model="editedEngine.settings[key].value"
                      type="checkbox"
                    />
                  </div>
                </div>
              </div>
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

<style scoped>
@import "@/assets/styles/variables.css";

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
  background-color: var(--selected-secondary);
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
  background-color: var(--light-white);
  color: #000000;
}
.engine-column.active input {
  background-color: var(--light-white);
  color: #000000;
}

.engine-column.active h2 {
  color: var(--light-white);
}
.engine-column.active .option-name {
  color: var(--light-white);
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
  font-size: 1rem;
}
input:focus {
  outline: 2px solid white;
  outline-offset: -1px;
}

.options {
  display: flex;
  flex-direction: column;
  flex-wrap: wrap;
  max-height: 50vh;
  overflow: auto;
  overflow-y: hidden;
}

.options::-webkit-scrollbar {
  width: 0.25rem;
}

.options::-webkit-scrollbar-track {
  background: #1e1e24;
}

.options::-webkit-scrollbar-thumb {
  background: #6649b8;
}

.option {
  margin: 10px;
}
</style>
