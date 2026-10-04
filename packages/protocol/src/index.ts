export type Health = {
  ok: true;
  version: string;
};

export type Problem = {
  type: string;
  title: string;
  status: number;
  detail?: string;
};
