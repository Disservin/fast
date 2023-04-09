<script lang="ts">
import { defineComponent } from "vue";

import {
  formatEval,
  formatNumber,
  formatTime,
  formatPv,
} from "@/ts/FormatInput";

import type { PV } from "@/ts/PrincipalVariation";

export default defineComponent({
  props: {
    engineLines: {
      type: Map<string, PV>,
      required: true,
    },
    color: {
      type: String,
      required: true,
    },
  },
  data() {
    return {
      engineLinesSorted: [] as PV[],
    };
  },
  watch: {
    engineLines: {
      handler: function (newVal, oldVal) {
        // move active line to top
        const lines: PV[] = Array.from(newVal.values());

        for (let i = 0; i < lines.length; i++) {
          if (lines[i].active) {
            const line = lines[i];
            lines.splice(i, 1);
            lines.unshift(line);
            break;
          }
        }
        this.engineLinesSorted = lines;
      },
      deep: true,
    },
  },
  methods: {
    formatEval,
    formatNumber,
    formatTime,
    formatPv,
  },
});
</script>

<template>
  <div class="container">
    <div
      :class="{ active: value.active }"
      class="line"
      v-for="value in engineLinesSorted"
    >
      <div class="eval-depth">
        <span :class="{ active: value.active }" class="stats">
          {{ formatEval(value.score, color) }}/{{ value.depth }}
        </span>
      </div>
      <div class="pv">
        <span class="pv-move" v-for="move in formatPv(value.pv)">
          {{ move }}&nbsp;</span
        >
      </div>
    </div>
  </div>
</template>

<style scoped>
@import "@/assets/styles/variables.css";
.container {
  overflow-y: auto;
}

.line {
  background-color: var(--bg-tertiary);
  margin: 10px;
  border: 1px solid #ccc;
  margin-bottom: 10px;
  padding: 10px;
}

.line span {
  font-family: Arial, sans-serif;
  font-size: 14px;
  line-height: 1.5;
  /* color: #333; */
}

.eval-depth {
  position: relative;
  top: 0;
  left: 0;
  width: 10%;
  text-align: left;
}

.eval-depth span.stats {
  font-size: 1rem;
  font-weight: bold;
}

.pv {
  width: 90%;
  text-align: left;
  display: inline-block;
}

span.active {
  color: #34d399;
}

.container::-webkit-scrollbar {
  width: 0.25rem;
}

.container::-webkit-scrollbar-track {
  background: #1e1e24;
}

.container::-webkit-scrollbar-thumb {
  background: #6649b8;
}
</style>
