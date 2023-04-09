<script lang="ts">
import { defineComponent } from "vue";

import { formatEval, formatNumber, formatTime } from "@/ts/FormatInput";

import type { PV } from "@/ts/PrincipalVariation";

export default defineComponent({
  props: {
    engine_lines: {
      type: Map<string, PV>,
      required: true,
    },
  },
  methods: {
    formatEval,
    formatNumber,
    formatTime,
  },
});
</script>

<template>
  <div>
    <div class="new-test" v-for="[key, value] in engine_lines" :key="key">
      <span>
        {{ formatEval(value.score) }}, {{ value.pv[0] }}, {{ value.depth }},
        {{ value.pv }}</span
      >
    </div>
  </div>
</template>

<style>
@import "../assets/styles/variables.css";
.container {
  overflow-y: auto;
}

.new-test {
  background-color: var(--bg-tertiary);
  margin: 5px;
  width: 100%;
}
</style>
