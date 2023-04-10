<script lang="ts">
import { defineComponent } from "vue";

import SmallBoard from "@/components/Analysis/Board/SmallBoard.vue";

import {
  formatEval,
  formatNumber,
  formatTime,
  formatPv,
} from "@/ts/FormatInput";

import { Chess } from "chess.js";

import type { PV } from "@/ts/PrincipalVariation";

export default defineComponent({
  components: {
    SmallBoard,
  },
  props: {
    engineLines: {
      type: Map<string, PV>,
      required: true,
    },
    fen: {
      type: String,
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
      showBoard: [-1, -1],
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
    getFenForMove(pvIndex: number, moveIndex: number): string {
      let fen = this.fen;
      const pv = this.engineLinesSorted[pvIndex].pv;

      let chess = new Chess(fen);

      for (let i = 0; i < moveIndex + 1; i++) {
        chess.move(pv[i].trim());
      }

      return chess.fen();
    },
  },
});
</script>

<template>
  <div class="container">
    <div
      :class="{ active: value.active }"
      class="line"
      v-for="(value, index) in engineLinesSorted"
    >
      <div class="eval-depth">
        <span :class="{ active: value.active }" class="stats">
          {{ formatEval(value.score, color) }}/{{ value.depth }}
        </span>
      </div>
      <div class="pv">
        <span class="pv-move" v-for="(move, indexMove) in formatPv(value.pv)">
          <span
            @mouseover="showBoard = [index, indexMove]"
            @mouseleave="showBoard = [-1, -1]"
          >
            {{ move }}&nbsp;
          </span>
          <SmallBoard
            v-if="showBoard[0] === index && showBoard[1] === indexMove"
            :fen="getFenForMove(index, indexMove)"
            :key="move"
            style="left: 0"
          />
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
@import "@/assets/styles/variables.css";
.container {
  overflow-y: scroll;
}

.line {
  background-color: var(--bg-tertiary);
  margin: 10px;
  border-radius: 5px;
  margin-bottom: 10px;
  padding: 10px;
}

.line span {
  font-family: Arial, sans-serif;
  font-size: 14px;
  line-height: 1.5;
}

.eval-depth {
  top: 0;
  left: 0;
  width: 10%;
  text-align: left;
  user-select: none;
  display: inline-block;
}

.eval-depth span.stats {
  font-size: 1rem;
  font-weight: bold;
}

.pv {
  display: inline-block;
  width: 90%;
  text-align: left;
  user-select: none;
  display: flex;
  flex-wrap: wrap;
}

.eval-depth span.active {
  color: #34d399;
}

.pv span.pv-move:hover {
  color: #34d399;
  background-color: #1e1e24;
  transform: scale(1);
}

.pv span.pv-move {
  padding: 5px;
  transition: transform 0.2s ease-in-out;
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
