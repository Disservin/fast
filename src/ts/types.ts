export interface Option {
  name: string;
  type: string;
  default: string;
  min: number;
  max: number;
  value: string;
}

export interface Engine {
  name: string;
  path: string;
  use: boolean;
  settings: Option[];
}
