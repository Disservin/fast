<template>
  <div>
    <div class="engine-stats">
      <div class="engine-stat">
        <!-- <span class="engine-stat-label"></span> -->
        <span class="engine-stat-value">{{
          formatEval(engine_info.score)
        }}</span>
      </div>
      <div class="engine-stat">
        <span class="engine-stat-value">{{
          formatTime(engine_info.time)
        }}</span>
      </div>
      <div class="engine-stat">
        <span class="engine-stat-value"
          >{{ formatNumber(engine_info.nps) }} nps</span
        >
      </div>
      <div class="engine-stat">
        <span class="engine-stat-value">{{
          formatNumber(engine_info.nodes)
        }}</span>
        <span class="engine-stat-label">nodes</span>
      </div>

      <div class="engine-stat">
        <span class="engine-stat-value">{{ engine_info.depth }}</span>
        <span class="engine-stat-label">depth</span>
      </div>

      <div class="engine-stat-advanced">
        <span class="engine-stat-value">{{ engine_info.tbhits }}</span>
        <span class="engine-stat-label">tbhits</span>
      </div>
      <div class="engine-stat-advanced">
        <span class="engine-stat-value">{{ engine_info.hashfull }}</span>
        <span class="engine-stat-label">hashfull</span>
      </div>
    </div>
  </div>
</template>

<script lang="ts">
import { defineComponent } from "vue";
import { formatEval, formatNumber, formatTime } from "@/ts/FormatInput";

export default defineComponent({
  props: ["engine_info"],
  methods: {
    formatEval,
    formatNumber,
    formatTime,
  },
});
</script>

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
