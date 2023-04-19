<script lang="ts">
import { defineComponent } from "vue";
import { formatEval, formatNumber, formatTime } from "@/ts/FormatInput";

export default defineComponent({
  props: ["engineInfo"],
  methods: {
    formatEval,
    formatNumber,
    formatTime,
  },
});
</script>

<template>
  <div>
    <div class="engine-stats">
      <div class="engine-stat">
        <span class="engine-stat-value"
          >{{ formatEval(engineInfo.score) }}
        </span>
      </div>
      <div class="engine-stat">
        <span class="engine-stat-value">{{ formatTime(engineInfo.time) }}</span>
      </div>
      <div class="engine-stat">
        <span class="engine-stat-value"
          >{{ formatNumber(engineInfo.nps) }} nps</span
        >
      </div>
      <div class="engine-stat">
        <span class="engine-stat-value">{{
          formatNumber(engineInfo.nodes)
        }}</span>
        <span class="engine-stat-label">nodes</span>
      </div>

      <div class="engine-stat">
        <span class="engine-stat-value">{{ engineInfo.depth }}</span>
        <span class="engine-stat-label">depth</span>
      </div>

      <div class="engine-stat-advanced">
        <span class="engine-stat-value">{{ engineInfo.tbhits }}</span>
        <span class="engine-stat-label">tbhits</span>
      </div>
      <div class="engine-stat-advanced">
        <span class="engine-stat-value">{{ engineInfo.hashfull }}</span>
        <span class="engine-stat-label">hashfull</span>
      </div>
    </div>
  </div>
</template>

<style>
.engine-stats {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-evenly;
  align-items: center;
  background-color: var(--bg-tertiary);
  color: white;
  padding: 10px;
  box-sizing: border-box;
  border-radius: 5px;
}

.engine-stats .advanced {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-evenly;
  align-items: center;
  background-color: var(--bg-tertiary);
  color: white;
}

.engine-stat {
  display: flex;
  align-items: center;
  margin: 5px;
}

.engine-stat-advanced {
  display: flex;
  align-items: center;
  margin: 5px;
}

.engine-stat-label {
  margin-left: 5px;
}

.engine-stat-value {
  font-size: 16px;
}
</style>
