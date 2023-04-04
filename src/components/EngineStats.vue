<template>
  <div>
    <div class="engine-stats">
      <div class="engine-stat">
        <span class="engine-stat-label">Nodes:</span>
        <span class="engine-stat-value">{{
          formatNumber(engine_info.nodes)
        }}</span>
      </div>
      <div class="engine-stat">
        <span class="engine-stat-label">Speed:</span>
        <span class="engine-stat-value"
          >{{ formatNumber(engine_info.nps) }} nps</span
        >
      </div>
      <div class="engine-stat">
        <span class="engine-stat-label">Time:</span>
        <span class="engine-stat-value">{{
          formatTime(engine_info.time)
        }}</span>
      </div>
      <div class="engine-stat">
        <span class="engine-stat-label">Depth:</span>
        <span class="engine-stat-value">{{ engine_info.depth }}</span>
      </div>
    </div>
  </div>
</template>

<script lang="ts">
import { defineComponent } from "vue";

export default defineComponent({
  props: ["engine_info"],
  methods: {
    formatNumber(numStr: string): string {
      const num = parseFloat(numStr);
      if (num >= 1e12) {
        return (num / 1e12).toFixed(1) + "T";
      } else if (num >= 1e9) {
        return (num / 1e9).toFixed(1) + "B";
      } else if (num >= 1e6) {
        return (num / 1e6).toFixed(1) + "M";
      } else if (num >= 1e3) {
        return (num / 1e3).toFixed(1) + "K";
      } else {
        return num.toFixed(0);
      }
    },
    formatTime(timeInMs: string): string {
      const time = parseInt(timeInMs);
      const hours = Math.floor(time / 3600000);
      const minutes = Math.floor((time - hours * 3600000) / 60000);
      const seconds = Math.floor(
        (time - hours * 3600000 - minutes * 60000) / 1000
      );
      return `${(hours < 10 ? "0" : "") + hours}:${
        (minutes < 10 ? "0" : "") + minutes
      }:${(seconds < 10 ? "0" : "") + seconds}`;
    },
  },
});
</script>

<style>
.engine-stats {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  align-items: center;
  background-color: #843b3b;
  color: white;
  padding: 10px;
  box-sizing: border-box;
  border-radius: 5px;
}

.engine-stat {
  display: flex;
  align-items: center;
  margin: 5px;
}

.engine-stat-label {
  font-weight: bold;
  margin-right: 5px;
}

.engine-stat-value {
  font-size: 16px;
}

.engine-stats {
  padding-left: 5rem;
}
</style>
