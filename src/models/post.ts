import * as Sequelize from 'sequelize';
import { DataTypes, Model, Optional } from 'sequelize';
import type { queue, queueId } from './queue';

export interface postAttributes {
  id?: number;
  post_id: string;
  last_updated?: string;
}

export type postPk = "id";
export type postId = post[postPk];
export type postOptionalAttributes = "id" | "last_updated";
export type postCreationAttributes = Optional<postAttributes, postOptionalAttributes>;

export class post extends Model<postAttributes, postCreationAttributes> implements postAttributes {
  id?: number;
  post_id!: string;
  last_updated?: string;

  // post hasMany queue via post_id
  queues!: queue[];
  getQueues!: Sequelize.HasManyGetAssociationsMixin<queue>;
  setQueues!: Sequelize.HasManySetAssociationsMixin<queue, queueId>;
  addQueue!: Sequelize.HasManyAddAssociationMixin<queue, queueId>;
  addQueues!: Sequelize.HasManyAddAssociationsMixin<queue, queueId>;
  createQueue!: Sequelize.HasManyCreateAssociationMixin<queue>;
  removeQueue!: Sequelize.HasManyRemoveAssociationMixin<queue, queueId>;
  removeQueues!: Sequelize.HasManyRemoveAssociationsMixin<queue, queueId>;
  hasQueue!: Sequelize.HasManyHasAssociationMixin<queue, queueId>;
  hasQueues!: Sequelize.HasManyHasAssociationsMixin<queue, queueId>;
  countQueues!: Sequelize.HasManyCountAssociationsMixin;

  static initModel(sequelize: Sequelize.Sequelize): typeof post {
    return post.init({
    id: {
      autoIncrement: true,
      type: DataTypes.INTEGER,
      allowNull: true,
      primaryKey: true
    },
    post_id: {
      type: DataTypes.TEXT,
      allowNull: false
    },
    last_updated: {
      type: DataTypes.TEXT,
      allowNull: true,
      defaultValue: "current_date"
    }
  }, {
    sequelize,
    tableName: 'post',
    timestamps: false
  });
  }
}
