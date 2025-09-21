import type { Sequelize } from "sequelize";
import { admin as _admin } from "./admin";
import type { adminAttributes, adminCreationAttributes } from "./admin";
import { post as _post } from "./post";
import type { postAttributes, postCreationAttributes } from "./post";
import { queue as _queue } from "./queue";
import type { queueAttributes, queueCreationAttributes } from "./queue";
import { tag as _tag } from "./tag";
import type { tagAttributes, tagCreationAttributes } from "./tag";

export {
  _admin as admin,
  _post as post,
  _queue as queue,
  _tag as tag,
};

export type {
  adminAttributes,
  adminCreationAttributes,
  postAttributes,
  postCreationAttributes,
  queueAttributes,
  queueCreationAttributes,
  tagAttributes,
  tagCreationAttributes,
};

export function initModels(sequelize: Sequelize) {
  const admin = _admin.initModel(sequelize);
  const post = _post.initModel(sequelize);
  const queue = _queue.initModel(sequelize);
  const tag = _tag.initModel(sequelize);

  queue.belongsTo(post, { as: "post", foreignKey: "post_id"});
  post.hasMany(queue, { as: "queues", foreignKey: "post_id"});

  return {
    admin: admin,
    post: post,
    queue: queue,
    tag: tag,
  };
}
