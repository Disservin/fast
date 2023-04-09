<script lang="ts">
export default {
  methods: {
    sendEngineCommand(command: string) {
      if (command === "go") {
        this.engineStatus = "running";
      } else if (command === "stop") {
        this.engineStatus = "stopped";
      } else if (command === "restart") {
        this.engineStatus = "stopped";
      }
      this.$emit("engine-command", command);

      localStorage.setItem("engineStatus", this.engineStatus);
    },
  },
  mounted() {
    this.engineStatus = localStorage.getItem("engineStatus") || "stopped";
  },
  data() {
    return {
      engineLog: "",
      // stopped, running
      engineStatus: "stopped",
    };
  },
};
</script>

<template lang="">
  <div>
    <v-row justify="center" class="mt-4 mb-4 mx-2">
      <v-col cols="4">
        <v-btn
          class="mr-2"
          outlined
          color="primary"
          block
          :disabled="engineStatus === 'running'"
          @click="sendEngineCommand('go')"
        >
          Go Infinite
        </v-btn>
      </v-col>
      <v-col cols="4">
        <v-btn
          class="mr-2"
          outlined
          color="primary"
          block
          :disabled="engineStatus === 'stopped'"
          @click="sendEngineCommand('stop')"
        >
          Stop
        </v-btn>
      </v-col>
      <v-col cols="4">
        <v-btn
          outlined
          color="primary"
          block
          :disabled="engineStatus === 'running'"
          @click="sendEngineCommand('restart')"
        >
          Restart
        </v-btn>
      </v-col>
    </v-row>
    <v-row>
      <v-col cols="12">
        <v-textarea
          no-resize
          hide-details
          v-model="engineLog"
          style="min-height: 100%"
          label="log"
        ></v-textarea
      ></v-col>
    </v-row>
  </div>
</template>

<style lang=""></style>
