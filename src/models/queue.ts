import * as Sequelize from 'sequelize';
import { DataTypes, Model, Optional } from 'sequelize';
import type { post, postId } from './post';

export interface queueAttributes {
  id?: number;
  post_id: number;
}

export type queuePk = "id";
export type queueId = queue[queuePk];
export type queueOptionalAttributes = "id";
export type queueCreationAttributes = Optional<queueAttributes, queueOptionalAttributes>;

export class queue extends Model<queueAttributes, queueCreationAttributes> implements queueAttributes {
  id?: number;
  post_id!: number;

  // queue belongsTo post via post_id
  post!: post;
  getPost!: Sequelize.BelongsToGetAssociationMixin<post>;
  setPost!: Sequelize.BelongsToSetAssociationMixin<post, postId>;
  createPost!: Sequelize.BelongsToCreateAssociationMixin<post>;

  static initModel(sequelize: Sequelize.Sequelize): typeof queue {
    return queue.init({
    id: {
      autoIncrement: true,
      type: DataTypes.INTEGER,
      allowNull: true,
      primaryKey: true
    },
    post_id: {
      type: DataTypes.INTEGER,
      allowNull: false,
      references: {
        model: 'post',
        key: 'id'
      }
    }
  }, {
    sequelize,
    tableName: 'queue',
    timestamps: false
  });
  }
}
